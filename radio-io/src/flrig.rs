use backon::{BackoffBuilder, ExponentialBuilder};
use radio_cat_rs::{Radio, xml_rpc::XmlRpcServerTask};
use std::{net::SocketAddr, time::Duration};
use tokio::{sync::oneshot, task::JoinHandle};
use tracing::{error, info, warn};

const RETRY_MIN_DELAY: Duration = Duration::from_secs(1);
const RETRY_MAX_DELAY: Duration = Duration::from_secs(10);

pub(crate) struct FlrigServer {
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl FlrigServer {
    pub(crate) fn start(radio_id: i64, radio: Radio, port: u16) -> Self {
        let (shutdown, shutdown_rx) = oneshot::channel();
        let task = tokio::spawn(run_server(radio_id, radio, port, shutdown_rx));
        Self {
            shutdown: Some(shutdown),
            task,
        }
    }

    pub(crate) async fn shutdown(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = self.task.await;
    }
}

async fn run_server(radio_id: i64, radio: Radio, port: u16, mut shutdown: oneshot::Receiver<()>) {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut backoff = retry_backoff().build();

    loop {
        let server = tokio::select! {
            _ = &mut shutdown => return,
            result = XmlRpcServerTask::bind(radio.clone(), address) => match result {
                Ok(server) => server,
                Err(error) => {
                    warn!(radio_id, %address, %error, "unable to bind FLRig XML-RPC listener");
                    if !wait_to_retry(radio_id, &mut backoff, &mut shutdown).await {
                        return;
                    }
                    continue;
                }
            }
        };

        backoff = retry_backoff().build();
        info!(radio_id, %address, "FLRig emulation available");
        let server_shutdown = server.shutdown_handle();
        let server_task = server.run();
        tokio::pin!(server_task);
        tokio::select! {
            _ = &mut shutdown => {
                server_shutdown.shutdown();
                if let Err(error) = server_task.await {
                    error!(radio_id, %address, %error, "FLRig XML-RPC server failed during shutdown");
                }
                return;
            }
            result = &mut server_task => {
                match result {
                    Ok(()) => warn!(radio_id, %address, "FLRig XML-RPC server stopped unexpectedly"),
                    Err(error) => error!(radio_id, %address, %error, "FLRig XML-RPC server failed"),
                }
            }
        }

        if !wait_to_retry(radio_id, &mut backoff, &mut shutdown).await {
            return;
        }
    }
}

async fn wait_to_retry(
    radio_id: i64,
    backoff: &mut impl Iterator<Item = Duration>,
    shutdown: &mut oneshot::Receiver<()>,
) -> bool {
    let delay = next_retry_delay(backoff);
    warn!(
        radio_id,
        retry_delay_ms = delay.as_millis(),
        "scheduled FLRig XML-RPC retry"
    );
    tokio::select! {
        _ = shutdown => false,
        _ = tokio::time::sleep(delay) => true,
    }
}

fn next_retry_delay(backoff: &mut impl Iterator<Item = Duration>) -> Duration {
    backoff.next().unwrap_or(RETRY_MAX_DELAY)
}

fn retry_backoff() -> ExponentialBuilder {
    ExponentialBuilder::default()
        .with_min_delay(RETRY_MIN_DELAY)
        .with_max_delay(RETRY_MAX_DELAY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_starts_at_one_second_and_caps_at_ten() {
        let mut backoff = retry_backoff().build();
        let delays = (0..6)
            .map(|_| next_retry_delay(&mut backoff))
            .collect::<Vec<_>>();
        assert_eq!(
            delays,
            vec![
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(4),
                Duration::from_secs(10),
                Duration::from_secs(10),
                Duration::from_secs(10),
            ]
        );
    }
}
