use crate::diagnostics::{self, Stage, StageTimer};
use std::{sync::Arc, time::Instant};
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct BlockingWork {
    permits: Arc<Semaphore>,
}

impl Default for BlockingWork {
    fn default() -> Self {
        Self {
            permits: Arc::new(Semaphore::new(2)),
        }
    }
}

impl BlockingWork {
    pub fn permits(&self) -> Arc<Semaphore> {
        Arc::clone(&self.permits)
    }

    pub async fn run<T: Send + 'static>(
        &self,
        operation: &'static str,
        work: impl FnOnce() -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        let queued = Instant::now();
        let timings = diagnostics::current_timings();
        let span = tracing::Span::current();
        let permit = Arc::clone(&self.permits)
            .acquire_owned()
            .await
            .map_err(|_| "blocking work is unavailable".to_string())?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let _span = span.enter();
            diagnostics::record(
                Stage::BlockingQueue,
                operation,
                queued.elapsed(),
                timings.as_deref(),
            );
            let _timer = StageTimer::with_timings(Stage::BlockingExecution, operation, timings);
            work()
        })
        .await
        .map_err(|_| format!("{operation} worker failed"))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn slow_work_does_not_block_http_or_timers() {
        use axum::{Router, routing::get};
        use tower::ServiceExt;
        let work = BlockingWork::default();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let task = tokio::spawn(async move {
            work.run("test_enumeration", move || {
                let _ = started_tx.send(());
                release_rx.recv().unwrap();
                Ok(())
            })
            .await
        });
        started_rx.await.unwrap();
        let response = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            Router::new()
                .route("/modes", get(|| async { "ok" }))
                .oneshot(
                    axum::http::Request::builder()
                        .uri("/modes")
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
        })
        .await;
        release_tx.send(()).unwrap();
        task.await.unwrap().unwrap();
        assert_eq!(response.unwrap().status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn cancellation_retains_the_permit_until_work_finishes() {
        let work = BlockingWork {
            permits: Arc::new(Semaphore::new(1)),
        };
        let work_clone = work.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let task = tokio::spawn(async move {
            work_clone
                .run("test", move || {
                    let _ = started_tx.send(());
                    release_rx.recv().unwrap();
                    Ok(())
                })
                .await
        });
        started_rx.await.unwrap();
        task.abort();
        let _ = task.await;
        assert_eq!(work.permits.available_permits(), 0);
        release_tx.send(()).unwrap();
        let permit =
            tokio::time::timeout(std::time::Duration::from_secs(2), work.permits.acquire())
                .await
                .unwrap()
                .unwrap();
        drop(permit);
    }
}
