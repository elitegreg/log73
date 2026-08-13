use crate::settings::{BackendSettings, RadioClientSettings};
use reqwest::{Client, StatusCode, Url};
use serde::{Deserialize, Serialize};
use std::{sync::mpsc::Sender, time::Duration};
use tokio::{sync::watch, task::JoinHandle, time::timeout};
use uuid::Uuid;

const MAX_RETRY_DELAY: Duration = Duration::from_secs(30);
const AUTH_RETRY_DELAY: Duration = Duration::from_secs(30);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendStatus {
    Registering,
    Registered { radio_id: i64 },
    Unavailable { retry_in: Duration },
    CredentialsRejected,
    LeaseReplaced,
}

impl BackendStatus {
    pub fn label(&self) -> String {
        match self {
            Self::Registering => "Backend: registering…".to_string(),
            Self::Registered { radio_id } => format!("Backend: registered as radio {radio_id}"),
            Self::Unavailable { retry_in } => format!(
                "Backend: unavailable; retrying in {} seconds",
                retry_in.as_secs()
            ),
            Self::CredentialsRejected => {
                "Backend: credentials rejected; retrying in 30 seconds".to_string()
            }
            Self::LeaseReplaced => "Backend: lease replaced; registering again…".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendEvent {
    pub status: BackendStatus,
}

#[derive(Debug, Serialize)]
struct RegisterRequest<'a> {
    radio_ws_url: &'a str,
    config: &'a radio_io::RadioSettings,
}

#[derive(Debug, Deserialize)]
struct Registration {
    radio_id: i64,
    lease_id: String,
    heartbeat_interval_seconds: u64,
    lease_timeout_seconds: u64,
}

#[derive(Debug, Serialize)]
struct LeaseRequest<'a> {
    radio_id: i64,
    lease_id: &'a str,
}

struct Lease {
    radio_id: i64,
    lease_id: String,
    heartbeat_interval: Duration,
}

pub struct RegistrationTask {
    cancel: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl RegistrationTask {
    pub async fn stop(self) {
        let _ = self.cancel.send(true);
        let mut task = self.task;
        if timeout(Duration::from_secs(3), &mut task).await.is_err() {
            task.abort();
            let _ = task.await;
        }
    }
}

pub fn start(
    settings: &RadioClientSettings,
    radio_ws_url: String,
    events: Sender<BackendEvent>,
) -> RegistrationTask {
    let (cancel, cancel_rx) = watch::channel(false);
    let settings = settings.clone();
    let task = tokio::spawn(async move {
        run(settings, radio_ws_url, events, cancel_rx).await;
    });
    RegistrationTask { cancel, task }
}

async fn run(
    settings: RadioClientSettings,
    radio_ws_url: String,
    events: Sender<BackendEvent>,
    mut cancel: watch::Receiver<bool>,
) {
    let client = Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .unwrap_or_else(|_| Client::new());
    let mut lease: Option<Lease> = None;
    let mut retry_delay = Duration::from_secs(1);

    loop {
        if *cancel.borrow() {
            break;
        }
        if let Some(active_lease) = lease.as_ref() {
            match wait_for_cancel_or_duration(&mut cancel, active_lease.heartbeat_interval).await {
                WaitResult::Cancelled => break,
                WaitResult::Elapsed => match heartbeat(&client, &settings, active_lease).await {
                    Ok(()) => continue,
                    Err(RequestFailure::LeaseReplaced) => {
                        emit(&events, BackendStatus::LeaseReplaced);
                        lease = None;
                        retry_delay = Duration::from_secs(1);
                    }
                    Err(RequestFailure::CredentialsRejected) => {
                        emit(&events, BackendStatus::CredentialsRejected);
                        lease = None;
                        if wait_for_cancel_or_duration(&mut cancel, AUTH_RETRY_DELAY).await
                            == WaitResult::Cancelled
                        {
                            break;
                        }
                    }
                    Err(RequestFailure::Unavailable) => {
                        emit(
                            &events,
                            BackendStatus::Unavailable {
                                retry_in: retry_delay,
                            },
                        );
                        lease = None;
                        if wait_for_cancel_or_duration(&mut cancel, retry_delay).await
                            == WaitResult::Cancelled
                        {
                            break;
                        }
                        retry_delay = next_retry_delay(retry_delay);
                    }
                },
            }
            continue;
        }

        emit(&events, BackendStatus::Registering);
        match register(&client, &settings, &radio_ws_url).await {
            Ok(new_lease) => {
                emit(
                    &events,
                    BackendStatus::Registered {
                        radio_id: new_lease.radio_id,
                    },
                );
                lease = Some(new_lease);
                retry_delay = Duration::from_secs(1);
            }
            Err(RequestFailure::CredentialsRejected) => {
                emit(&events, BackendStatus::CredentialsRejected);
                if wait_for_cancel_or_duration(&mut cancel, AUTH_RETRY_DELAY).await
                    == WaitResult::Cancelled
                {
                    break;
                }
            }
            Err(RequestFailure::LeaseReplaced) | Err(RequestFailure::Unavailable) => {
                emit(
                    &events,
                    BackendStatus::Unavailable {
                        retry_in: retry_delay,
                    },
                );
                if wait_for_cancel_or_duration(&mut cancel, retry_delay).await
                    == WaitResult::Cancelled
                {
                    break;
                }
                retry_delay = next_retry_delay(retry_delay);
            }
        }
    }

    if let Some(active_lease) = lease {
        let _ = timeout(
            Duration::from_secs(3),
            offline(&client, &settings, &active_lease),
        )
        .await;
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WaitResult {
    Cancelled,
    Elapsed,
}

async fn wait_for_cancel_or_duration(
    cancel: &mut watch::Receiver<bool>,
    duration: Duration,
) -> WaitResult {
    if *cancel.borrow() {
        return WaitResult::Cancelled;
    }
    match timeout(duration, cancel.changed()).await {
        Ok(Ok(())) | Ok(Err(_)) => WaitResult::Cancelled,
        Err(_) => WaitResult::Elapsed,
    }
}

fn emit(events: &Sender<BackendEvent>, status: BackendStatus) {
    let _ = events.send(BackendEvent { status });
}

fn next_retry_delay(delay: Duration) -> Duration {
    delay.saturating_mul(2).min(MAX_RETRY_DELAY)
}

async fn register(
    client: &Client,
    settings: &RadioClientSettings,
    radio_ws_url: &str,
) -> Result<Lease, RequestFailure> {
    let url = endpoint(
        &settings.backend,
        &format!("radio-clients/{}", settings.client_instance_id),
    )?;
    let response = authorized(client.put(url), &settings.backend)
        .json(&RegisterRequest {
            radio_ws_url,
            config: &settings.radio,
        })
        .send()
        .await
        .map_err(|_| RequestFailure::Unavailable)?;
    classify(response.status())?;
    let registration = response
        .json::<Registration>()
        .await
        .map_err(|_| RequestFailure::Unavailable)?;
    if registration.radio_id <= 0
        || Uuid::parse_str(&registration.lease_id).is_err()
        || registration.heartbeat_interval_seconds == 0
        || registration.lease_timeout_seconds == 0
        || registration.heartbeat_interval_seconds > registration.lease_timeout_seconds
    {
        return Err(RequestFailure::Unavailable);
    }
    Ok(Lease {
        radio_id: registration.radio_id,
        lease_id: registration.lease_id,
        heartbeat_interval: Duration::from_secs(registration.heartbeat_interval_seconds),
    })
}

async fn heartbeat(
    client: &Client,
    settings: &RadioClientSettings,
    lease: &Lease,
) -> Result<(), RequestFailure> {
    let resource = format!("radio-clients/{}/heartbeat", settings.client_instance_id);
    lease_request(
        client.post(endpoint(&settings.backend, &resource)?),
        &settings.backend,
        lease,
    )
    .await
}

async fn offline(
    client: &Client,
    settings: &RadioClientSettings,
    lease: &Lease,
) -> Result<(), RequestFailure> {
    let resource = format!("radio-clients/{}/offline", settings.client_instance_id);
    lease_request(
        client.post(endpoint(&settings.backend, &resource)?),
        &settings.backend,
        lease,
    )
    .await
}

async fn lease_request(
    request: reqwest::RequestBuilder,
    backend: &BackendSettings,
    lease: &Lease,
) -> Result<(), RequestFailure> {
    let response = authorized(request, backend)
        .json(&LeaseRequest {
            radio_id: lease.radio_id,
            lease_id: &lease.lease_id,
        })
        .send()
        .await
        .map_err(|_| RequestFailure::Unavailable)?;
    classify(response.status())
}

fn endpoint(backend: &BackendSettings, resource: &str) -> Result<Url, RequestFailure> {
    let mut url = Url::parse(backend.base_url.trim()).map_err(|_| RequestFailure::Unavailable)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(RequestFailure::Unavailable);
    }
    url.set_path(&format!(
        "{}/api/{resource}",
        url.path().trim_end_matches('/')
    ));
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn authorized(
    request: reqwest::RequestBuilder,
    backend: &BackendSettings,
) -> reqwest::RequestBuilder {
    if !backend.username.is_empty() && !backend.password.is_empty() {
        request.basic_auth(&backend.username, Some(&backend.password))
    } else {
        request
    }
}

#[derive(Debug, Clone, Copy)]
enum RequestFailure {
    CredentialsRejected,
    LeaseReplaced,
    Unavailable,
}

fn classify(status: StatusCode) -> Result<(), RequestFailure> {
    if status.is_success() {
        Ok(())
    } else if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        Err(RequestFailure::CredentialsRejected)
    } else if status == StatusCode::CONFLICT {
        Err(RequestFailure::LeaseReplaced)
    } else {
        Err(RequestFailure::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_uses_api_path_without_embedded_credentials() {
        let backend = BackendSettings {
            base_url: "http://localhost:7300/base".to_string(),
            username: String::new(),
            password: String::new(),
        };
        assert_eq!(
            endpoint(&backend, "radio-clients/id").unwrap().as_str(),
            "http://localhost:7300/base/api/radio-clients/id"
        );
        assert!(
            endpoint(
                &BackendSettings {
                    base_url: "http://u:p@localhost".to_string(),
                    ..backend
                },
                "x"
            )
            .is_err()
        );
    }

    #[test]
    fn retry_delay_is_bounded() {
        assert_eq!(
            next_retry_delay(Duration::from_secs(1)),
            Duration::from_secs(2)
        );
        assert_eq!(
            next_retry_delay(Duration::from_secs(30)),
            Duration::from_secs(30)
        );
    }
}
