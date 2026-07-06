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
use rand_core::OsRng;
use tracing::debug;

use crate::{AppState, db::AuthConfig};

pub async fn basic_auth(
    State(app_state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let Ok(config) = app_state.db.auth_config().await else {
        debug!(method = %request.method(), uri = %request.uri(), "request failed basic authentication config lookup");
        return unauthorized(request);
    };

    if !is_auth_enabled(&config) || authorized(&request, &config) {
        return next.run(request).await;
    }

    debug!(method = %request.method(), uri = %request.uri(), "request failed basic authentication");
    unauthorized(request)
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

fn unauthorized(_request: Request<Body>) -> Response {
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

fn authorized(request: &Request<Body>, config: &AuthConfig) -> bool {
    let Some(value) = request.headers().get(header::AUTHORIZATION) else {
        return false;
    };
    let Ok(value) = value.to_str() else {
        return false;
    };
    let Some(encoded) = value.strip_prefix("Basic ") else {
        return false;
    };
    let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(encoded) else {
        return false;
    };
    let Ok(credentials) = String::from_utf8(decoded) else {
        return false;
    };
    let Some((username, password)) = credentials.split_once(':') else {
        return false;
    };

    if username != config.login_user.trim() {
        return false;
    }

    verify_password_hash(password, &config.login_password)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
