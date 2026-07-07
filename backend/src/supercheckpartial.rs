use crate::{
    db::{Contact, contact_adif_value},
    log_cache::LogCacheProcessor,
    radio::ServerMessage,
    scoring::ContestScoringModule,
};
use std::{
    collections::HashSet,
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
    sync::{Arc, Mutex},
};
use tokio::sync::broadcast;

#[derive(Clone, Debug, Default)]
pub struct SuperCheckPartial {
    inner: Arc<Mutex<SuperCheckPartialState>>,
    events: Option<broadcast::Sender<ServerMessage>>,
}

#[derive(Debug, Default)]
struct SuperCheckPartialState {
    callsigns: Vec<String>,
    seen: HashSet<String>,
}

impl SuperCheckPartial {
    pub fn load_file(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let file = File::open(path.as_ref())?;
        let reader = BufReader::new(file);
        let cache = Self::default();
        let mut callsigns = Vec::new();

        for line in reader.lines() {
            let line = line?;
            let callsign = line.trim();
            if callsign.is_empty() || callsign.starts_with('!') || callsign.starts_with('#') {
                continue;
            }
            callsigns.push(callsign.to_string());
        }

        let _ = cache.insert_callsigns(callsigns);
        Ok(cache)
    }

    pub fn with_events(mut self, events: broadcast::Sender<ServerMessage>) -> Self {
        self.events = Some(events);
        self
    }

    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .expect("supercheckpartial mutex poisoned")
            .callsigns
            .len()
    }

    pub fn callsigns(&self) -> Vec<String> {
        self.inner
            .lock()
            .expect("supercheckpartial mutex poisoned")
            .callsigns
            .clone()
    }

    pub fn insert_callsigns<I, S>(&self, callsigns: I) -> Vec<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut state = self.inner.lock().expect("supercheckpartial mutex poisoned");
        let mut added = Vec::new();

        for callsign in callsigns {
            let Some(normalized) = normalize_callsign(callsign.as_ref()) else {
                continue;
            };
            if state.seen.insert(normalized.clone()) {
                state.callsigns.push(normalized.clone());
                added.push(normalized);
            }
        }

        added
    }

    fn insert_contact_callsigns(&self, contacts: &[Contact]) -> Vec<String> {
        self.insert_callsigns(contacts.iter().filter_map(contact_callsign))
    }

    fn broadcast_added_callsigns(&self, callsigns: Vec<String>) {
        if callsigns.is_empty() {
            return;
        }
        if let Some(events) = &self.events {
            let _ = events.send(ServerMessage::SupercheckpartialUpdate { callsigns });
        }
    }
}

impl LogCacheProcessor for SuperCheckPartial {
    fn on_log_loaded(
        &self,
        _log_id: i64,
        _module: Arc<ContestScoringModule>,
        contacts: &mut [Contact],
    ) {
        self.broadcast_added_callsigns(self.insert_contact_callsigns(contacts));
    }

    fn on_contacts_upserted(
        &self,
        _log_id: i64,
        _module: Arc<ContestScoringModule>,
        _contacts: &mut [Contact],
        committed_contacts: &[Contact],
        _previous_contacts: &[Option<Contact>],
    ) -> Vec<Contact> {
        self.broadcast_added_callsigns(self.insert_contact_callsigns(committed_contacts));
        Vec::new()
    }

    fn on_contact_deleted(
        &self,
        _log_id: i64,
        _module: Arc<ContestScoringModule>,
        _contacts: &mut [Contact],
        _deleted_contact: &Contact,
    ) -> Vec<Contact> {
        Vec::new()
    }
}

fn contact_callsign(contact: &Contact) -> Option<&str> {
    contact_adif_value(contact, "CALL").and_then(serde_json::Value::as_str)
}

fn normalize_callsign(value: &str) -> Option<String> {
    let normalized = value.trim().to_uppercase();
    if normalized.is_empty() {
        return None;
    }
    Some(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::build_contact;
    use serde_json::{Map, json};

    fn contact(call: &str) -> Contact {
        build_contact(
            Map::new(),
            Map::from_iter([("CALL".to_string(), json!(call))]),
        )
    }

    fn recv_update(events: &mut broadcast::Receiver<ServerMessage>) -> Vec<String> {
        match events.try_recv().expect("expected websocket event") {
            ServerMessage::SupercheckpartialUpdate { callsigns } => callsigns,
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn insert_callsigns_normalizes_and_dedupes() {
        let cache = SuperCheckPartial::default();

        let added = cache.insert_callsigns([" k1abc ", "K1ABC", "", "n0call"]);

        assert_eq!(added, vec!["K1ABC", "N0CALL"]);
        assert_eq!(cache.callsigns(), vec!["K1ABC", "N0CALL"]);
    }

    #[test]
    fn on_log_loaded_adds_contact_callsigns_and_broadcasts_only_new_entries() {
        let (tx, mut rx) = broadcast::channel(8);
        let cache = SuperCheckPartial::default().with_events(tx);
        let mut contacts = vec![contact("k1abc"), contact("K1ABC"), contact("n0call")];

        cache.on_log_loaded(7, Arc::new(ContestScoringModule::default()), &mut contacts);

        assert_eq!(cache.callsigns(), vec!["K1ABC", "N0CALL"]);
        assert_eq!(recv_update(&mut rx), vec!["K1ABC", "N0CALL"]);
        assert!(matches!(
            rx.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
    }

    #[test]
    fn on_contacts_upserted_broadcasts_only_truly_new_callsigns() {
        let (tx, mut rx) = broadcast::channel(8);
        let cache = SuperCheckPartial::default().with_events(tx);
        let mut all_contacts = vec![contact("K1ABC")];
        let committed_contacts = vec![contact("K1ABC"), contact("W1AW")];

        cache.on_log_loaded(
            7,
            Arc::new(ContestScoringModule::default()),
            &mut all_contacts,
        );
        let _ = recv_update(&mut rx);

        let changed = cache.on_contacts_upserted(
            7,
            Arc::new(ContestScoringModule::default()),
            &mut all_contacts,
            &committed_contacts,
            &[None, None],
        );

        assert!(changed.is_empty());
        assert_eq!(recv_update(&mut rx), vec!["W1AW"]);
        assert!(matches!(
            rx.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
        assert_eq!(cache.callsigns(), vec!["K1ABC", "W1AW"]);
    }

    #[test]
    fn existing_master_scp_entries_do_not_rebroadcast_when_seen_in_contacts() {
        let (tx, mut rx) = broadcast::channel(8);
        let cache = SuperCheckPartial::default().with_events(tx);
        let added = cache.insert_callsigns(["K1ABC"]);
        assert_eq!(added, vec!["K1ABC"]);

        let mut contacts = vec![contact("k1abc")];
        cache.on_log_loaded(7, Arc::new(ContestScoringModule::default()), &mut contacts);

        assert!(matches!(
            rx.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
        assert_eq!(cache.callsigns(), vec!["K1ABC"]);
    }
}
