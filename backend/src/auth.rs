use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::Engine;

const USERNAME: &str = "log73";
const PASSWORD: &str = "hamradio";

pub async fn basic_auth(request: Request<Body>, next: Next) -> Response {
    if authorized(&request) {
        return next.run(request).await;
    }

    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Basic realm=\"Log73\"")],
        "authentication required",
    )
        .into_response()
}

fn authorized(request: &Request<Body>) -> bool {
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

    credentials == format!("{USERNAME}:{PASSWORD}")
}
