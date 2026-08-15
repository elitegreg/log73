//! Bidirectional text messaging through FLDigi's XML-RPC interface.

use dxr::{FaultResponse, MethodCall, MethodResponse, Value};
use reqwest::{Client, Url};
use std::borrow::Cow;
use std::{fmt, time::Duration};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
    time::{MissedTickBehavior, timeout},
};
use tracing::{debug, warn};

pub const DEFAULT_FLDIGI_ENDPOINT: &str = "http://127.0.0.1:7362/RPC2";
pub const DEFAULT_FLDIGI_POLL_INTERVAL: Duration = Duration::from_millis(500);
pub const DEFAULT_FLDIGI_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

const CHANNEL_CAPACITY: usize = 32;

/// Connection settings for an FLDigi XML-RPC interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FldigiConfig {
    pub endpoint: String,
    pub poll_interval: Duration,
    pub request_timeout: Duration,
}

impl Default for FldigiConfig {
    fn default() -> Self {
        Self {
            endpoint: DEFAULT_FLDIGI_ENDPOINT.to_string(),
            poll_interval: DEFAULT_FLDIGI_POLL_INTERVAL,
            request_timeout: DEFAULT_FLDIGI_REQUEST_TIMEOUT,
        }
    }
}

/// Work accepted by the FLDigi interface's outbound channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FldigiCommand {
    /// Adds the text to FLDigi's TX buffer and starts transmitting it.
    Transmit(String),
    /// Clears FLDigi's receive text buffer.
    ClearReceiveBuffer,
}

/// An XML-RPC failure reported asynchronously by the interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FldigiError {
    pub method: &'static str,
    pub message: String,
}

impl fmt::Display for FldigiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "FLDigi {} failed: {}", self.method, self.message)
    }
}

impl std::error::Error for FldigiError {}

/// Channel-based FLDigi interface and its worker lifecycle.
///
/// Send [`FldigiCommand`] values through [`commands`](Self::commands), and
/// consume newly received text from [`received`](Self::received). XML-RPC
/// failures are delivered separately through [`errors`](Self::errors), so the
/// receive channel contains only over-the-air text.
pub struct FldigiInterface {
    pub commands: mpsc::Sender<FldigiCommand>,
    pub received: mpsc::Receiver<String>,
    pub errors: mpsc::Receiver<FldigiError>,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

struct XmlRpcClient {
    endpoint: Url,
    http: Client,
}

impl FldigiInterface {
    /// Starts an FLDigi interface. The endpoint is validated before the worker
    /// is spawned; FLDigi itself does not need to be running yet.
    pub fn start(config: FldigiConfig) -> Result<Self, String> {
        if config.poll_interval.is_zero() {
            return Err("FLDigi poll interval must be greater than zero".to_string());
        }
        if config.request_timeout.is_zero() {
            return Err("FLDigi request timeout must be greater than zero".to_string());
        }

        let endpoint = Url::parse(config.endpoint.trim())
            .map_err(|error| format!("invalid FLDigi XML-RPC endpoint: {error}"))?;
        if endpoint.scheme() != "http" {
            return Err("FLDigi XML-RPC endpoint must use http".to_string());
        }
        if endpoint.host().is_none() {
            return Err("FLDigi XML-RPC endpoint must include a host".to_string());
        }

        let client = XmlRpcClient {
            endpoint,
            http: Client::new(),
        };
        let (commands, command_rx) = mpsc::channel(CHANNEL_CAPACITY);
        let (received_tx, received) = mpsc::channel(CHANNEL_CAPACITY);
        let (errors_tx, errors) = mpsc::channel(CHANNEL_CAPACITY);
        let (shutdown, shutdown_rx) = oneshot::channel();
        let task = tokio::spawn(run_interface(
            client,
            config.poll_interval,
            config.request_timeout,
            command_rx,
            received_tx,
            errors_tx,
            shutdown_rx,
        ));

        Ok(Self {
            commands,
            received,
            errors,
            shutdown: Some(shutdown),
            task: Some(task),
        })
    }

    /// Stops the worker and waits for it to finish.
    pub async fn shutdown(mut self) {
        self.request_shutdown();
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }

    fn request_shutdown(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
    }
}

impl Drop for FldigiInterface {
    fn drop(&mut self) {
        self.request_shutdown();
    }
}

async fn run_interface(
    client: XmlRpcClient,
    poll_interval: Duration,
    request_timeout: Duration,
    mut commands: mpsc::Receiver<FldigiCommand>,
    received: mpsc::Sender<String>,
    errors: mpsc::Sender<FldigiError>,
    mut shutdown: oneshot::Receiver<()>,
) {
    let mut poll = tokio::time::interval(poll_interval);
    poll.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut rx_position = 0;
    let mut poll_error_active = false;

    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            command = commands.recv() => {
                let Some(command) = command else {
                    break;
                };
                let clears_receive_buffer = matches!(command, FldigiCommand::ClearReceiveBuffer);
                match apply_command(&client, request_timeout, command).await {
                    Ok(()) if clears_receive_buffer => rx_position = 0,
                    Ok(()) => {}
                    Err(error) => {
                        warn!(%error, "FLDigi command failed");
                        report_error(&errors, error);
                    }
                }
            }
            _ = poll.tick() => {
                match receive_new_text(&client, request_timeout, &mut rx_position).await {
                    Ok(Some(text)) => {
                        poll_error_active = false;
                        tokio::select! {
                            _ = &mut shutdown => break,
                            result = received.send(text) => {
                                if result.is_err() {
                                    debug!("FLDigi receive channel closed");
                                    break;
                                }
                            }
                        }
                    }
                    Ok(None) => poll_error_active = false,
                    Err(error) => {
                        if !poll_error_active {
                            warn!(%error, "FLDigi receive polling failed");
                            report_error(&errors, error);
                            poll_error_active = true;
                        }
                    }
                }
            }
        }
    }
}

async fn apply_command(
    client: &XmlRpcClient,
    request_timeout: Duration,
    command: FldigiCommand,
) -> Result<(), FldigiError> {
    match command {
        FldigiCommand::Transmit(text) => {
            // FLDigi interprets caret-R as "return to receive" once the queued
            // text has been sent.
            let text = format!("{text}^r");
            let _: Value = call(client, request_timeout, "text.add_tx", (text,)).await?;
            let _: Value = call(client, request_timeout, "main.tx", Value::Nil).await?;
            Ok(())
        }
        FldigiCommand::ClearReceiveBuffer => {
            // Some FLDigi versions reply with an empty string instead of the
            // documented XML-RPC nil. The command has no meaningful result.
            let _: Value = call(client, request_timeout, "text.clear_rx", Value::Nil).await?;
            Ok(())
        }
    }
}

async fn receive_new_text(
    client: &XmlRpcClient,
    request_timeout: Duration,
    rx_position: &mut i32,
) -> Result<Option<String>, FldigiError> {
    let length: i32 = call(client, request_timeout, "text.get_rx_length", Value::Nil).await?;
    if length < 0 {
        return Err(FldigiError {
            method: "text.get_rx_length",
            message: format!("returned a negative receive-buffer length ({length})"),
        });
    }
    if length < *rx_position {
        // The FLDigi UI or another XML-RPC client cleared the buffer.
        *rx_position = 0;
    }
    if length == *rx_position {
        return Ok(None);
    }

    let unread_length = length.saturating_sub(*rx_position);
    let bytes: Vec<u8> = call(
        client,
        request_timeout,
        "text.get_rx",
        (*rx_position, unread_length),
    )
    .await?;
    *rx_position = length;

    if bytes.is_empty() {
        Ok(None)
    } else {
        Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
    }
}

async fn call<P, R>(
    client: &XmlRpcClient,
    request_timeout: Duration,
    method: &'static str,
    params: P,
) -> Result<R, FldigiError>
where
    P: dxr::TryToParams,
    R: dxr::TryFromValue,
{
    match timeout(request_timeout, client.call(method, params)).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(message)) => Err(FldigiError { method, message }),
        Err(_) => Err(FldigiError {
            method,
            message: format!("request timed out after {request_timeout:?}"),
        }),
    }
}

impl XmlRpcClient {
    async fn call<P, R>(&self, method: &'static str, params: P) -> Result<R, String>
    where
        P: dxr::TryToParams,
        R: dxr::TryFromValue,
    {
        let request = MethodCall {
            name: Cow::Borrowed(method),
            params: params.try_to_params().map_err(|error| error.to_string())?,
        };
        let body = format!(
            "<?xml version=\"1.0\"?>\n{}\n",
            request.to_xml().map_err(|error| error.to_string())?
        );
        let response = self
            .http
            .post(self.endpoint.clone())
            .header(reqwest::header::CONTENT_TYPE, "text/xml")
            .body(body)
            .send()
            .await
            .map_err(|error| error.to_string())?;
        let status = response.status();
        let contents = response.text().await.map_err(|error| error.to_string())?;
        if !status.is_success() {
            return Err(format!("HTTP {status}: {contents}"));
        }
        if let Ok(fault) = FaultResponse::from_xml(&contents) {
            return Err(fault.fault.to_string());
        }
        let response = MethodResponse::from_xml(&contents).map_err(|error| error.to_string())?;
        R::try_from_value(&response.value).map_err(|error| error.to_string())
    }
}

fn report_error(errors: &mpsc::Sender<FldigiError>, error: FldigiError) {
    if let Err(send_error) = errors.try_send(error) {
        debug!(%send_error, "unable to deliver FLDigi error");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, extract::State, http::StatusCode, routing::post};
    use dxr::{MethodCall, MethodResponse, Value};
    use std::sync::Arc;
    use tokio::{net::TcpListener, sync::Mutex, time::Instant};

    #[derive(Default)]
    struct MockFldigi {
        rx: Mutex<Vec<u8>>,
        calls: Mutex<Vec<(String, Vec<Value>)>>,
    }

    struct MockServer {
        endpoint: String,
        state: Arc<MockFldigi>,
        shutdown: Option<oneshot::Sender<()>>,
        task: JoinHandle<()>,
    }

    impl MockServer {
        async fn start(initial_rx: &[u8]) -> Self {
            let state = Arc::new(MockFldigi {
                rx: Mutex::new(initial_rx.to_vec()),
                calls: Mutex::new(Vec::new()),
            });
            let app = Router::new()
                .route("/RPC2", post(handle_rpc))
                .with_state(state.clone());
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind mock FLDigi");
            let address = listener.local_addr().expect("mock FLDigi address");
            let (shutdown, shutdown_rx) = oneshot::channel();
            let task = tokio::spawn(async move {
                axum::serve(listener, app)
                    .with_graceful_shutdown(async move {
                        let _ = shutdown_rx.await;
                    })
                    .await
                    .expect("run mock FLDigi");
            });

            Self {
                endpoint: format!("http://{address}/RPC2"),
                state,
                shutdown: Some(shutdown),
                task,
            }
        }

        async fn shutdown(mut self) {
            if let Some(shutdown) = self.shutdown.take() {
                let _ = shutdown.send(());
            }
            let _ = self.task.await;
        }
    }

    async fn handle_rpc(
        State(state): State<Arc<MockFldigi>>,
        body: String,
    ) -> Result<String, (StatusCode, String)> {
        let request = MethodCall::from_xml(&body)
            .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
        let method = request.name.into_owned();
        let params = request.params;
        state
            .calls
            .lock()
            .await
            .push((method.clone(), params.clone()));

        let value = match method.as_str() {
            "text.get_rx_length" => {
                if params.as_slice() != [Value::Nil] {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        "text.get_rx_length expected nil".to_string(),
                    ));
                }
                let length = state.rx.lock().await.len();
                Value::Integer(i32::try_from(length).expect("mock RX length fits in i32"))
            }
            "text.get_rx" => {
                let [Value::Integer(start), Value::Integer(length)] = params.as_slice() else {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        "text.get_rx expected two integers".to_string(),
                    ));
                };
                let start = usize::try_from(*start)
                    .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
                let length = usize::try_from(*length)
                    .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
                let rx = state.rx.lock().await;
                let end = start.saturating_add(length).min(rx.len());
                Value::Base64(rx.get(start..end).unwrap_or_default().to_vec())
            }
            "text.clear_rx" => {
                if params.as_slice() != [Value::Nil] {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        "text.clear_rx expected nil".to_string(),
                    ));
                }
                state.rx.lock().await.clear();
                Value::String("cleared".to_string())
            }
            "text.add_tx" => Value::String("queued".to_string()),
            "main.tx" => {
                if params.as_slice() != [Value::Nil] {
                    return Err((StatusCode::BAD_REQUEST, "main.tx expected nil".to_string()));
                }
                Value::String("transmitting".to_string())
            }
            _ => {
                return Err((
                    StatusCode::NOT_FOUND,
                    format!("unknown XML-RPC method {method}"),
                ));
            }
        };

        MethodResponse { value }
            .to_xml()
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
    }

    fn test_config(endpoint: String) -> FldigiConfig {
        FldigiConfig {
            endpoint,
            poll_interval: Duration::from_millis(10),
            request_timeout: Duration::from_secs(1),
        }
    }

    #[tokio::test]
    async fn sends_receives_and_clears_text() {
        let server = MockServer::start(b"CQ").await;
        let mut interface = FldigiInterface::start(test_config(server.endpoint.clone()))
            .expect("start FLDigi interface");

        assert_eq!(receive_with_timeout(&mut interface).await, "CQ");
        server.state.rx.lock().await.extend_from_slice(b" TEST");
        assert_eq!(receive_with_timeout(&mut interface).await, " TEST");

        interface
            .commands
            .send(FldigiCommand::Transmit("hello".to_string()))
            .await
            .expect("queue transmit");
        wait_for_call(&server.state, "main.tx").await;

        let calls = server.state.calls.lock().await;
        let add_tx = calls
            .iter()
            .position(|(method, _)| method == "text.add_tx")
            .expect("text.add_tx called");
        let main_tx = calls
            .iter()
            .position(|(method, _)| method == "main.tx")
            .expect("main.tx called");
        assert!(add_tx < main_tx);
        assert_eq!(calls[add_tx].1, vec![Value::String("hello^r".to_string())]);
        drop(calls);

        interface
            .commands
            .send(FldigiCommand::ClearReceiveBuffer)
            .await
            .expect("queue RX clear");
        wait_for_call(&server.state, "text.clear_rx").await;
        server.state.rx.lock().await.extend_from_slice(b"NEW");
        assert_eq!(receive_with_timeout(&mut interface).await, "NEW");

        interface.shutdown().await;
        server.shutdown().await;
    }

    #[tokio::test]
    async fn reports_connection_errors_without_closing_command_channel() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("reserve unused port");
        let address = listener.local_addr().expect("unused address");
        drop(listener);

        let mut interface = FldigiInterface::start(test_config(format!("http://{address}/RPC2")))
            .expect("start FLDigi interface");
        let error = timeout(Duration::from_secs(1), interface.errors.recv())
            .await
            .expect("receive error before timeout")
            .expect("error channel remains open");

        assert_eq!(error.method, "text.get_rx_length");
        assert!(!interface.commands.is_closed());
        interface.shutdown().await;
    }

    #[test]
    fn validates_configuration_before_spawning() {
        let config = FldigiConfig {
            endpoint: "file:///tmp/fldigi".to_string(),
            ..FldigiConfig::default()
        };
        assert!(FldigiInterface::start(config).is_err());

        let config = FldigiConfig {
            poll_interval: Duration::ZERO,
            ..FldigiConfig::default()
        };
        assert!(FldigiInterface::start(config).is_err());
    }

    async fn receive_with_timeout(interface: &mut FldigiInterface) -> String {
        timeout(Duration::from_secs(1), interface.received.recv())
            .await
            .expect("receive FLDigi text before timeout")
            .expect("FLDigi receive channel remains open")
    }

    async fn wait_for_call(state: &MockFldigi, expected: &str) {
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            if state
                .calls
                .lock()
                .await
                .iter()
                .any(|(method, _)| method == expected)
            {
                return;
            }
            assert!(Instant::now() < deadline, "{expected} was not called");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}
