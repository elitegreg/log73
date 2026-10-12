use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use hmac::{Hmac, Mac};
use rand_core::{OsRng, RngCore};
use sha2::Sha256;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{Semaphore, watch};
use tracing::{Instrument, debug};

use crate::{
    db::AuthConfig,
    diagnostics::{Stage, StageTimer},
};

const CACHE_TTL: Duration = Duration::from_secs(30 * 60);
const CACHE_CAPACITY: usize = 256;
const MAX_PENDING: usize = 32;
const MAX_AUTHORIZATION_BYTES: usize = 8192;
type CacheKey = [u8; 32];
type Verifier = dyn Fn(&str, &str) -> bool + Send + Sync;
type Clock = dyn Fn() -> Instant + Send + Sync;

#[derive(Clone)]
pub struct AuthService {
    inner: Arc<AuthInner>,
}

struct AuthInner {
    state: Mutex<AuthState>,
    secret: [u8; 32],
    permits: Arc<Semaphore>,
    verifier: Arc<Verifier>,
    clock: Arc<Clock>,
}

struct AuthState {
    config: AuthConfig,
    generation: u64,
    cache: HashMap<CacheKey, CacheEntry>,
    pending: HashMap<CacheKey, watch::Sender<Option<AuthDecision>>>,
    access_counter: u64,
}

struct CacheEntry {
    verified_at: Instant,
    last_access: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AuthDecision {
    Allowed,
    Unauthorized,
    Busy,
    Failed,
}

impl AuthService {
    pub fn new(config: AuthConfig) -> Self {
        Self::with_verifier(
            config,
            Arc::new(verify_password_hash),
            Arc::new(Instant::now),
        )
    }

    fn with_verifier(config: AuthConfig, verifier: Arc<Verifier>, clock: Arc<Clock>) -> Self {
        let mut secret = [0; 32];
        OsRng.fill_bytes(&mut secret);
        Self {
            inner: Arc::new(AuthInner {
                state: Mutex::new(AuthState {
                    config,
                    generation: 0,
                    cache: HashMap::new(),
                    pending: HashMap::new(),
                    access_counter: 0,
                }),
                secret,
                permits: Arc::new(Semaphore::new(2)),
                verifier,
                clock,
            }),
        }
    }

    pub fn replace_config(&self, config: AuthConfig) {
        let mut state = self.inner.state.lock().expect("auth state mutex poisoned");
        if state.config.login_user.trim() != config.login_user.trim()
            || state.config.login_password != config.login_password
        {
            state.generation = state.generation.wrapping_add(1);
            state.cache.clear();
            // Wake waiters immediately; a previous verification may still be running.
            for sender in state.pending.values() {
                sender.send_replace(Some(AuthDecision::Unauthorized));
            }
            state.pending.clear();
        }
        state.config = config;
    }

    fn key(&self, username: &str, password: &str) -> CacheKey {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.inner.secret).expect("valid HMAC key");
        mac.update(&(username.len() as u64).to_be_bytes());
        mac.update(username.as_bytes());
        mac.update(password.as_bytes());
        mac.finalize().into_bytes().into()
    }

    async fn authorize(&self, authorization: Option<&axum::http::HeaderValue>) -> AuthDecision {
        let (mut receiver, generation) = {
            let mut state = self.inner.state.lock().expect("auth state mutex poisoned");
            if !is_auth_enabled(&state.config) {
                return AuthDecision::Allowed;
            }
            let Some(credentials) = credentials(authorization) else {
                return AuthDecision::Unauthorized;
            };
            let Some((username, password)) = credentials.split_once(':') else {
                return AuthDecision::Unauthorized;
            };
            if username != state.config.login_user.trim() {
                return AuthDecision::Unauthorized;
            }
            let key = self.key(username, password);
            let now = (self.inner.clock)();
            state
                .cache
                .retain(|_, entry| now.duration_since(entry.verified_at) < CACHE_TTL);
            state.access_counter = state.access_counter.wrapping_add(1);
            let last_access = state.access_counter;
            if let Some(entry) = state.cache.get_mut(&key) {
                entry.last_access = last_access;
                debug!(cache_hit = true, "authentication cache lookup");
                return AuthDecision::Allowed;
            }
            let generation = state.generation;
            if let Some(sender) = state.pending.get(&key) {
                (sender.subscribe(), generation)
            } else {
                if state.pending.len() >= MAX_PENDING {
                    return AuthDecision::Busy;
                }
                let (sender, receiver) = watch::channel(None);
                state.pending.insert(key, sender.clone());
                let service = self.clone();
                let candidate = password.to_string();
                let stored_hash = state.config.login_password.clone();
                // The task owns its lifetime, so cancellation cannot strand other waiters.
                tokio::spawn(
                    async move {
                        service
                            .verify(key, generation, candidate, stored_hash, sender)
                            .await;
                    }
                    .instrument(tracing::Span::current()),
                );
                (receiver, generation)
            }
        };
        let decision = loop {
            if let Some(decision) = *receiver.borrow_and_update() {
                break decision;
            }
            if receiver.changed().await.is_err() {
                break AuthDecision::Failed;
            }
        };
        let state = self.inner.state.lock().expect("auth state mutex poisoned");
        if state.generation != generation {
            AuthDecision::Unauthorized
        } else {
            decision
        }
    }

    async fn verify(
        &self,
        key: CacheKey,
        generation: u64,
        candidate: String,
        stored_hash: String,
        sender: watch::Sender<Option<AuthDecision>>,
    ) {
        let inner = Arc::clone(&self.inner);
        let decision = match Arc::clone(&inner.permits).acquire_owned().await {
            Ok(permit) => {
                if inner
                    .state
                    .lock()
                    .expect("auth state mutex poisoned")
                    .generation
                    != generation
                {
                    sender.send_replace(Some(AuthDecision::Unauthorized));
                    return;
                }
                let span = tracing::Span::current();
                let verifier = Arc::clone(&inner.verifier);
                match tokio::task::spawn_blocking(move || {
                    let _permit = permit;
                    let _span = span.enter();
                    let start = Instant::now();
                    let valid = verifier(&candidate, &stored_hash);
                    debug!(
                        cache_hit = false,
                        verification_ms = start.elapsed().as_secs_f64() * 1000.0,
                        "password verification completed"
                    );
                    valid
                })
                .await
                {
                    Ok(true) => AuthDecision::Allowed,
                    Ok(false) => AuthDecision::Unauthorized,
                    Err(_) => AuthDecision::Failed,
                }
            }
            Err(_) => AuthDecision::Failed,
        };
        let mut state = inner.state.lock().expect("auth state mutex poisoned");
        if state.generation != generation {
            sender.send_replace(Some(AuthDecision::Unauthorized));
            return;
        }
        state.pending.remove(&key);
        if decision == AuthDecision::Allowed {
            if state.cache.len() >= CACHE_CAPACITY
                && let Some(oldest) = state
                    .cache
                    .iter()
                    .min_by_key(|(_, entry)| entry.last_access)
                    .map(|(key, _)| *key)
            {
                state.cache.remove(&oldest);
            }
            let last_access = state.access_counter;
            state.cache.insert(
                key,
                CacheEntry {
                    verified_at: (inner.clock)(),
                    last_access,
                },
            );
        }
        sender.send_replace(Some(decision));
    }

    pub async fn hash_password(&self, password: String) -> Result<String, String> {
        let queued = Instant::now();
        let timings = crate::diagnostics::current_timings();
        let span = tracing::Span::current();
        let permit = Arc::clone(&self.inner.permits)
            .acquire_owned()
            .await
            .map_err(|_| "password hashing is unavailable".to_string())?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let _span = span.enter();
            crate::diagnostics::record(
                Stage::BlockingQueue,
                "hash_password",
                queued.elapsed(),
                timings.as_deref(),
            );
            let _timer =
                StageTimer::with_timings(Stage::BlockingExecution, "hash_password", timings);
            hash_password(&password)
        })
        .await
        .map_err(|_| "password hashing worker failed".to_string())?
    }
}

fn credentials(authorization: Option<&axum::http::HeaderValue>) -> Option<String> {
    let value = authorization?;
    if value.as_bytes().len() > MAX_AUTHORIZATION_BYTES {
        return None;
    }
    let encoded = value.to_str().ok()?.strip_prefix("Basic ")?;
    String::from_utf8(
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .ok()?,
    )
    .ok()
}

pub async fn basic_auth(
    State(service): State<AuthService>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let timer = StageTimer::new(Stage::Authentication, "basic_auth");
    let decision = service
        .authorize(request.headers().get(header::AUTHORIZATION))
        .await;
    drop(timer);
    match decision {
        AuthDecision::Allowed => {
            let _timer = StageTimer::new(Stage::Handler, "http_handler");
            next.run(request).await
        }
        AuthDecision::Unauthorized => unauthorized(),
        AuthDecision::Busy => (
            StatusCode::SERVICE_UNAVAILABLE,
            [(header::RETRY_AFTER, "1")],
            "authentication busy; retry shortly",
        )
            .into_response(),
        AuthDecision::Failed => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "authentication worker failed",
        )
            .into_response(),
    }
}

pub fn hash_password(password: &str) -> Result<String, String> {
    if password.is_empty() {
        return Err("login password cannot be empty".to_string());
    }

    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| format!("failed to hash login password: {error}"))
}

pub(crate) fn verify_password_hash(candidate: &str, stored_hash: &str) -> bool {
    if stored_hash.is_empty() {
        return candidate.is_empty();
    }

    let Ok(parsed_hash) = PasswordHash::new(stored_hash) else {
        return false;
    };

    Argon2::default()
        .verify_password(candidate.as_bytes(), &parsed_hash)
        .is_ok()
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Basic realm=\"Log73\"")],
        "authentication required",
    )
        .into_response()
}

fn is_auth_enabled(config: &AuthConfig) -> bool {
    !config.login_user.trim().is_empty() && !config.login_password.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn config() -> AuthConfig {
        AuthConfig {
            login_user: "greg".to_string(),
            login_password: "hash-v1".to_string(),
        }
    }

    fn authorization(password: &str) -> axum::http::HeaderValue {
        axum::http::HeaderValue::from_str(&format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("greg:{password}"))
        ))
        .unwrap()
    }

    fn fake_service() -> (AuthService, Arc<AtomicUsize>, Arc<Mutex<Instant>>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let verifier_calls = Arc::clone(&calls);
        let clock = Arc::new(Mutex::new(Instant::now()));
        let verifier_clock = Arc::clone(&clock);
        let service = AuthService::with_verifier(
            config(),
            Arc::new(move |password, _| {
                verifier_calls.fetch_add(1, Ordering::SeqCst);
                password != "wrong"
            }),
            Arc::new(move || *verifier_clock.lock().unwrap()),
        );
        (service, calls, clock)
    }

    #[tokio::test]
    async fn success_is_cached_with_absolute_expiry_and_lru_eviction() {
        let (service, calls, clock) = fake_service();
        let header = authorization("secret");
        assert_eq!(
            service.authorize(Some(&header)).await,
            AuthDecision::Allowed
        );
        assert_eq!(
            service.authorize(Some(&header)).await,
            AuthDecision::Allowed
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        *clock.lock().unwrap() += CACHE_TTL - Duration::from_secs(1);
        assert_eq!(
            service.authorize(Some(&header)).await,
            AuthDecision::Allowed
        );
        *clock.lock().unwrap() += Duration::from_secs(1);
        assert_eq!(
            service.authorize(Some(&header)).await,
            AuthDecision::Allowed
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        for index in 0..CACHE_CAPACITY - 1 {
            assert_eq!(
                service
                    .authorize(Some(&authorization(&format!("password-{index}"))))
                    .await,
                AuthDecision::Allowed
            );
        }
        assert_eq!(
            service.authorize(Some(&header)).await,
            AuthDecision::Allowed
        );
        service.authorize(Some(&authorization("extra"))).await;
        assert_eq!(
            service.inner.state.lock().unwrap().cache.len(),
            CACHE_CAPACITY
        );
        assert_eq!(
            service.authorize(Some(&header)).await,
            AuthDecision::Allowed
        );
        let count = calls.load(Ordering::SeqCst);
        service.authorize(Some(&authorization("password-0"))).await;
        assert_eq!(calls.load(Ordering::SeqCst), count + 1);
    }

    #[tokio::test]
    async fn invalid_headers_and_disabled_login_do_not_verify_and_failures_are_not_cached() {
        let (service, calls, _) = fake_service();
        for value in ["Bearer secret", "Basic !!!", "Basic bm9jb2xvbg=="] {
            assert_eq!(
                service
                    .authorize(Some(&axum::http::HeaderValue::from_str(value).unwrap()))
                    .await,
                AuthDecision::Unauthorized
            );
        }
        let oversized = axum::http::HeaderValue::from_str(&format!(
            "Basic {}",
            "x".repeat(MAX_AUTHORIZATION_BYTES)
        ))
        .unwrap();
        assert_eq!(
            service.authorize(Some(&oversized)).await,
            AuthDecision::Unauthorized
        );
        assert_eq!(service.authorize(None).await, AuthDecision::Unauthorized);
        let other_user = axum::http::HeaderValue::from_static("Basic b3RoZXI6c2VjcmV0");
        assert_eq!(
            service.authorize(Some(&other_user)).await,
            AuthDecision::Unauthorized
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let wrong = authorization("wrong");
        for _ in 0..2 {
            assert_eq!(
                service.authorize(Some(&wrong)).await,
                AuthDecision::Unauthorized
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        service.replace_config(AuthConfig {
            login_user: "greg".to_string(),
            login_password: String::new(),
        });
        assert_eq!(service.authorize(None).await, AuthDecision::Allowed);
        assert_eq!(
            service.authorize(Some(&oversized)).await,
            AuthDecision::Allowed
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn effective_credential_changes_invalidate_but_unchanged_config_preserves_cache() {
        let (service, calls, _) = fake_service();
        let header = authorization("secret");
        service.authorize(Some(&header)).await;
        service.replace_config(config());
        service.authorize(Some(&header)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        service.replace_config(AuthConfig {
            login_user: " greg ".to_string(),
            ..config()
        });
        service.authorize(Some(&header)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        service.replace_config(AuthConfig {
            login_password: "hash-v2".to_string(),
            ..config()
        });
        service.authorize(Some(&header)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        service.replace_config(AuthConfig {
            login_user: "other".to_string(),
            ..config()
        });
        assert_eq!(
            service.authorize(Some(&header)).await,
            AuthDecision::Unauthorized
        );
        assert!(service.inner.state.lock().unwrap().cache.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn concurrent_verification_is_shared_and_does_not_block_unrelated_http() {
        use axum::{Router, routing::get};
        use tower::ServiceExt;
        let calls = Arc::new(AtomicUsize::new(0));
        let verifier_calls = Arc::clone(&calls);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let started = Mutex::new(Some(started_tx));
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release = Mutex::new(release_rx);
        let service = AuthService::with_verifier(
            config(),
            Arc::new(move |_, _| {
                verifier_calls.fetch_add(1, Ordering::SeqCst);
                if let Some(sender) = started.lock().unwrap().take() {
                    let _ = sender.send(());
                }
                release.lock().unwrap().recv().unwrap();
                true
            }),
            Arc::new(Instant::now),
        );
        let mut tasks = Vec::new();
        for _ in 0..8 {
            let service = service.clone();
            tasks.push(tokio::spawn(async move {
                service.authorize(Some(&authorization("secret"))).await
            }));
        }
        started_rx.await.unwrap();
        let cancelled = tasks.pop().unwrap();
        cancelled.abort();
        assert!(cancelled.await.unwrap_err().is_cancelled());
        let response = tokio::time::timeout(Duration::from_secs(2), async {
            tokio::time::sleep(Duration::from_millis(1)).await;
            Router::new()
                .route("/modes", get(|| async { "ok" }))
                .oneshot(
                    Request::builder()
                        .uri("/modes")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
        })
        .await;
        release_tx.send(()).unwrap();
        for task in tasks {
            assert_eq!(task.await.unwrap(), AuthDecision::Allowed);
        }
        assert_eq!(response.unwrap().status(), StatusCode::OK);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn credential_change_wakes_pending_requests_and_prevents_stale_success() {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let started = Mutex::new(Some(started_tx));
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release = Mutex::new(release_rx);
        let service = AuthService::with_verifier(
            config(),
            Arc::new(move |_, _| {
                if let Some(sender) = started.lock().unwrap().take() {
                    let _ = sender.send(());
                }
                release.lock().unwrap().recv().unwrap();
                true
            }),
            Arc::new(Instant::now),
        );
        let old = service.clone();
        let task = tokio::spawn(async move { old.authorize(Some(&authorization("secret"))).await });
        started_rx.await.unwrap();
        service.replace_config(AuthConfig {
            login_password: "new-hash".to_string(),
            ..config()
        });
        assert_eq!(task.await.unwrap(), AuthDecision::Unauthorized);
        release_tx.send(()).unwrap();
        let _permit = tokio::time::timeout(
            Duration::from_secs(2),
            Arc::clone(&service.inner.permits).acquire_many_owned(2),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(service.inner.state.lock().unwrap().cache.is_empty());
    }

    #[tokio::test]
    async fn pending_work_is_bounded_and_http_overload_has_retry_after() {
        use axum::{Router, middleware, routing::get};
        use tower::ServiceExt;
        let (service, _, _) = fake_service();
        let permits = Arc::clone(&service.inner.permits)
            .acquire_many_owned(2)
            .await
            .unwrap();
        let mut tasks = Vec::new();
        for index in 0..MAX_PENDING {
            let service = service.clone();
            tasks.push(tokio::spawn(async move {
                service
                    .authorize(Some(&authorization(&format!("secret-{index}"))))
                    .await
            }));
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if service.inner.state.lock().unwrap().pending.len() == MAX_PENDING {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(middleware::from_fn_with_state(service, basic_auth));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header(header::AUTHORIZATION, authorization("one-too-many"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        drop(permits);
        for task in tasks {
            assert_eq!(task.await.unwrap(), AuthDecision::Allowed);
        }
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers()[header::RETRY_AFTER], "1");
    }

    #[tokio::test]
    async fn verifier_panic_fails_closed_and_releases_pending_work() {
        let service = AuthService::with_verifier(
            config(),
            Arc::new(|_, _| panic!("injected failure")),
            Arc::new(Instant::now),
        );
        assert_eq!(
            service.authorize(Some(&authorization("secret"))).await,
            AuthDecision::Failed
        );
        let state = service.inner.state.lock().unwrap();
        assert!(state.cache.is_empty());
        assert!(state.pending.is_empty());
        assert_eq!(service.inner.permits.available_permits(), 2);
    }

    #[tokio::test]
    async fn basic_auth_challenge_and_cached_authorization_cover_api_assets_and_websockets() {
        use axum::{Router, middleware, routing::get};
        use tower::ServiceExt;
        let (service, calls, _) = fake_service();
        let app = Router::new()
            .route("/api/modes", get(|| async { "ok" }))
            .route("/ws", get(|| async { "websocket handshake reached" }))
            .route("/radiows", get(|| async { "radio handshake reached" }))
            .fallback(|| async { "asset" })
            .layer(middleware::from_fn_with_state(service, basic_auth));
        for path in ["/api/modes", "/ws", "/radiows", "/static/app.js"] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            assert_eq!(
                response.headers()[header::WWW_AUTHENTICATE],
                "Basic realm=\"Log73\""
            );
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .header(header::AUTHORIZATION, authorization("secret"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn hash_password_uses_random_salt() {
        let left = hash_password("secret").expect("password hash should be generated");
        let right = hash_password("secret").expect("password hash should be generated");

        assert_ne!(left, right);
    }

    #[test]
    fn hash_password_rejects_empty_password() {
        let error = hash_password("").expect_err("empty password should be rejected");

        assert_eq!(error, "login password cannot be empty");
    }

    #[test]
    fn verify_password_hash_accepts_matching_password() {
        let hash = hash_password("secret").expect("password hash should be generated");

        assert!(verify_password_hash("secret", &hash));
        assert!(!verify_password_hash("not-secret", &hash));
    }

    #[test]
    fn auth_is_disabled_when_username_or_password_is_blank() {
        assert!(!is_auth_enabled(&AuthConfig {
            login_user: String::new(),
            login_password: "hash".to_string(),
        }));
        assert!(!is_auth_enabled(&AuthConfig {
            login_user: "greg".to_string(),
            login_password: String::new(),
        }));
    }
}
