use axum::{body::Body, http::Request, middleware::Next, response::Response};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tracing::{Instrument, debug, info, warn};
use uuid::Uuid;

const SLOW_OPERATION: Duration = Duration::from_millis(200);

#[derive(Default)]
pub struct Timings {
    auth: AtomicU64,
    handler: AtomicU64,
    db_queue: AtomicU64,
    db_execution: AtomicU64,
    blocking_queue: AtomicU64,
    blocking_execution: AtomicU64,
    cache_hits: AtomicU64,
    cache_misses: AtomicU64,
    cache_loads: AtomicU64,
    db_read_commands: AtomicU64,
    db_write_commands: AtomicU64,
}

tokio::task_local! {
    static REQUEST_TIMINGS: Arc<Timings>;
}

#[derive(Clone, Copy)]
pub enum Stage {
    Authentication,
    Handler,
    DatabaseQueue,
    DatabaseExecution,
    BlockingQueue,
    BlockingExecution,
}

impl Stage {
    fn name(self) -> &'static str {
        match self {
            Self::Authentication => "authentication",
            Self::Handler => "handler",
            Self::DatabaseQueue => "database_queue",
            Self::DatabaseExecution => "database_execution",
            Self::BlockingQueue => "blocking_queue",
            Self::BlockingExecution => "blocking_execution",
        }
    }

    fn counter(self, timings: &Timings) -> &AtomicU64 {
        match self {
            Self::Authentication => &timings.auth,
            Self::Handler => &timings.handler,
            Self::DatabaseQueue => &timings.db_queue,
            Self::DatabaseExecution => &timings.db_execution,
            Self::BlockingQueue => &timings.blocking_queue,
            Self::BlockingExecution => &timings.blocking_execution,
        }
    }
}

pub fn current_timings() -> Option<Arc<Timings>> {
    REQUEST_TIMINGS.try_with(Arc::clone).ok()
}

pub fn cache_hit() {
    if let Some(timings) = current_timings() {
        timings.cache_hits.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn cache_miss() {
    if let Some(timings) = current_timings() {
        timings.cache_misses.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn cache_load(count: u64, timings: Option<&Timings>) {
    if count != 0 {
        if let Some(timings) = timings {
            timings.cache_loads.fetch_add(count, Ordering::Relaxed);
        }
        tracing::debug!(count, "database contact cache loaded");
    }
}

pub fn database_command(read: bool, timings: Option<&Timings>) {
    if let Some(timings) = timings {
        if read {
            timings.db_read_commands.fetch_add(1, Ordering::Relaxed);
        } else {
            timings.db_write_commands.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub fn record(stage: Stage, operation: &'static str, elapsed: Duration, timings: Option<&Timings>) {
    if let Some(timings) = timings {
        stage.counter(timings).fetch_add(
            elapsed.as_nanos().min(u64::MAX as u128) as u64,
            Ordering::Relaxed,
        );
    }
    let stage = stage.name();
    let elapsed_ms = elapsed.as_secs_f64() * 1000.0;
    if elapsed >= SLOW_OPERATION {
        warn!(stage, operation, elapsed_ms, "slow backend operation");
    } else {
        debug!(stage, operation, elapsed_ms, "backend operation completed");
    }
}

pub struct StageTimer {
    stage: Stage,
    operation: &'static str,
    start: Instant,
    timings: Option<Arc<Timings>>,
}

impl StageTimer {
    pub fn new(stage: Stage, operation: &'static str) -> Self {
        Self::with_timings(stage, operation, current_timings())
    }

    pub fn with_timings(
        stage: Stage,
        operation: &'static str,
        timings: Option<Arc<Timings>>,
    ) -> Self {
        Self {
            stage,
            operation,
            start: Instant::now(),
            timings,
        }
    }
}

impl Drop for StageTimer {
    fn drop(&mut self) {
        record(
            self.stage,
            self.operation,
            self.start.elapsed(),
            self.timings.as_deref(),
        );
    }
}

pub async fn request_timing(request: Request<Body>, next: Next) -> Response {
    let span = tracing::info_span!(
        "http_request", request_id = %Uuid::new_v4(),
        method = %request.method(), path = %request.uri().path(),
    );
    let timings = Arc::new(Timings::default());
    REQUEST_TIMINGS
        .scope(Arc::clone(&timings), async move {
            let start = Instant::now();
            let response = next.run(request).await;
            let latency_ms = start.elapsed().as_secs_f64() * 1000.0;
            let milliseconds =
                |counter: &AtomicU64| counter.load(Ordering::Relaxed) as f64 / 1_000_000.0;
            info!(
                status = response.status().as_u16(),
                latency_ms,
                auth_ms = milliseconds(&timings.auth),
                handler_ms = milliseconds(&timings.handler),
                db_queue_ms = milliseconds(&timings.db_queue),
                db_execution_ms = milliseconds(&timings.db_execution),
                blocking_queue_ms = milliseconds(&timings.blocking_queue),
                blocking_execution_ms = milliseconds(&timings.blocking_execution),
                db_cache_hits = timings.cache_hits.load(Ordering::Relaxed),
                db_cache_misses = timings.cache_misses.load(Ordering::Relaxed),
                db_cache_loads = timings.cache_loads.load(Ordering::Relaxed),
                db_read_commands = timings.db_read_commands.load(Ordering::Relaxed),
                db_write_commands = timings.db_write_commands.load(Ordering::Relaxed),
                "request completed",
            );
            if start.elapsed() >= SLOW_OPERATION {
                warn!(latency_ms, "slow backend request");
            }
            response
        })
        .instrument(span)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, middleware, routing::get};
    use std::{io::Write, sync::Mutex};
    use tower::ServiceExt;
    use tracing::instrument::WithSubscriber;
    use tracing_subscriber::prelude::*;

    #[derive(Clone)]
    struct LogBuffer(Arc<Mutex<Vec<u8>>>);

    impl Write for LogBuffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn request_summary_has_correlation_and_timings_without_query_or_credentials() {
        let buffer = LogBuffer(Arc::new(Mutex::new(Vec::new())));
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .without_time()
                .with_writer(move || writer.clone()),
        );
        let service = crate::auth::AuthService::new(crate::disabled_auth_config());
        let app = Router::new()
            .route(
                "/modes",
                get(|| async {
                    let timings = current_timings();
                    let span = tracing::Span::current();
                    tokio::task::spawn_blocking(move || {
                        let _span = span.enter();
                        record(
                            Stage::DatabaseQueue,
                            "test",
                            Duration::from_millis(12),
                            timings.as_deref(),
                        );
                    })
                    .await
                    .unwrap();
                    "ok"
                }),
            )
            .layer(middleware::from_fn_with_state(
                service,
                crate::auth::basic_auth,
            ))
            .layer(middleware::from_fn(request_timing));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/modes?password=query-secret")
                    .header("authorization", "Basic header-secret")
                    .header("cookie", "session=cookie-secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .with_subscriber(subscriber)
            .await
            .unwrap();
        assert!(response.status().is_success());
        let logs = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
        for field in [
            "request_id=",
            "method=GET",
            "path=/modes",
            "status=200",
            "latency_ms=",
            "auth_ms=",
            "handler_ms=",
            "db_queue_ms=12",
            "db_execution_ms=",
            "blocking_execution_ms=",
        ] {
            assert!(logs.contains(field), "missing {field}: {logs}");
        }
        for secret in [
            "query-secret",
            "header-secret",
            "cookie-secret",
            "password=",
        ] {
            assert!(!logs.contains(secret), "sensitive value leaked");
        }
    }
}
