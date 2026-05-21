use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use tracing::debug;

use crate::{AppState, db::AuthConfig};

pub async fn basic_auth(
    State(app_state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let Ok(config) = app_state.db.auth_config() else {
        debug!(method = %request.method(), uri = %request.uri(), "request failed basic authentication config lookup");
        return unauthorized(request);
    };

    if !is_auth_enabled(&config) || authorized(&request, &config) {
        return next.run(request).await;
    }

    debug!(method = %request.method(), uri = %request.uri(), "request failed basic authentication");
    unauthorized(request)
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

    credentials == format!("{}:{}", config.login_user.trim(), config.login_password)
}
