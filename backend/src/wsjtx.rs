use crate::{
    adif,
    bands::BandCatalog,
    contest_rules::ContestRulesStore,
    db::{Database, RadioConfig},
    log_cache::LogCache,
    radio::{RadioState, ServerMessage},
    scoring::IncrementalScoreTracker,
    validation,
};
use radio_io::WsjtXEvent;
pub use radio_io::WsjtXTargetState;
use tokio::sync::broadcast;
use tracing::{debug, error, warn};

#[derive(Clone)]
pub struct WsjtXManager {
    inner: radio_io::WsjtXManager,
}

#[derive(Clone)]
struct WsjtXIngestor {
    db: Database,
    rules: ContestRulesStore,
    bands: BandCatalog,
    log_cache: LogCache,
    scoring: IncrementalScoreTracker,
    events: broadcast::Sender<ServerMessage>,
}

impl WsjtXManager {
    pub fn new(
        db: Database,
        rules: ContestRulesStore,
        bands: BandCatalog,
        log_cache: LogCache,
        scoring: IncrementalScoreTracker,
        events: broadcast::Sender<ServerMessage>,
    ) -> Self {
        let inner = radio_io::WsjtXManager::new();
        let ingestor = WsjtXIngestor {
            db,
            rules,
            bands,
            log_cache,
            scoring,
            events,
        };
        tokio::spawn(run_ingestor(inner.subscribe_events(), ingestor));
        Self { inner }
    }

    pub async fn acquire(
        &self,
        radio_id: i64,
        logger_id: &str,
        log_id: i64,
        config: RadioConfig,
        initial_state: Option<RadioState>,
        updates: broadcast::Receiver<RadioState>,
    ) -> Result<WsjtXTargetState, String> {
        self.inner
            .acquire(radio_id, logger_id, log_id, config, initial_state, updates)
            .await
    }

    pub async fn release(&self, radio_id: i64, logger_id: &str) {
        self.inner.release(radio_id, logger_id).await;
    }

    pub async fn set_target(
        &self,
        radio_id: i64,
        logger_id: &str,
        enabled: bool,
    ) -> Result<WsjtXTargetState, String> {
        self.inner.set_target(radio_id, logger_id, enabled).await
    }

    pub fn subscribe_targets(&self) -> broadcast::Receiver<WsjtXTargetState> {
        self.inner.subscribe_targets()
    }

    pub async fn reload_config(&self, radio_id: i64, config: RadioConfig) {
        self.inner.reload_config(radio_id, config).await;
    }
}

async fn run_ingestor(mut source: broadcast::Receiver<WsjtXEvent>, ingestor: WsjtXIngestor) {
    loop {
        match source.recv().await {
            Ok(WsjtXEvent::LoggedAdif {
                radio_id,
                log_id,
                text,
            }) => {
                if let Err(message) = ingestor.ingest(radio_id, log_id, &text).await {
                    ingestor.emit_error(radio_id, log_id, message);
                }
            }
            Ok(WsjtXEvent::Error {
                radio_id,
                log_id,
                message,
            }) => ingestor.emit_error(radio_id, log_id, message),
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                warn!(skipped, "WSJT-X backend event bridge lagged");
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

impl WsjtXIngestor {
    async fn ingest(&self, radio_id: i64, log_id: i64, text: &str) -> Result<(), String> {
        let log = self
            .db
            .log(log_id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("WSJT-X target log {log_id} was not found"))?;
        let rules = self
            .rules
            .get(&log.contest_id)
            .ok_or_else(|| format!("Unknown contest: {}", log.contest_id))?;
        let contact = adif::import_wsjtx_contact(rules, text)
            .map_err(|error| format!("Unable to import WSJT-X ADIF: {}", error.error))?;
        validation::validate_contacts(
            &self.db,
            &self.rules,
            self.bands.snapshot().as_ref(),
            log_id,
            std::slice::from_ref(&contact),
        )
        .await
        .map_err(|error| format!("Unable to validate WSJT-X contact: {error}"))?;
        let result = self
            .log_cache
            .upsert_contacts(log_id, vec![contact])
            .await
            .map_err(|error| format!("Unable to save WSJT-X contact: {error}"))?;
        for contact in result.contacts.into_iter().chain(result.changed_contacts) {
            let _ = self.events.send(ServerMessage::LogEntry { contact });
        }
        let totals = self.scoring.totals(log_id).unwrap_or_default();
        let _ = self.events.send(ServerMessage::ScoreUpdate {
            log_id,
            qso_count: totals.qso_count,
            multipliers: totals.multipliers,
            bonus_points: totals.bonus_points,
            total_score: totals.score,
        });
        debug!(radio_id, log_id, "saved WSJT-X Logged ADIF contact");
        Ok(())
    }

    fn emit_error(&self, radio_id: i64, log_id: i64, message: String) {
        error!(radio_id, log_id, %message, "WSJT-X error");
        let _ = self.events.send(ServerMessage::WsjtXError {
            radio_id,
            log_id,
            message,
        });
    }
}
