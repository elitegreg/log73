use axum::body::Body;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../dist"]
struct DistAssets;

#[derive(RustEmbed)]
#[folder = "../docs"]
struct DocsAssets;

pub async fn static_handler(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    if path == "help" || path.starts_with("help/") {
        let help_relative_path = path
            .strip_prefix("help/")
            .unwrap_or("")
            .trim_start_matches('/');
        return match help_asset_key(help_relative_path).and_then(docs_asset_response) {
            Some(response) => response,
            None => (StatusCode::NOT_FOUND, "not found").into_response(),
        };
    }

    let app_path = if path.is_empty() { "index.html" } else { path };

    match dist_asset_response(app_path).or_else(|| dist_asset_response("index.html")) {
        Some(response) => response,
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

fn help_asset_key(path: &str) -> Option<String> {
    let requested_path = if path.is_empty() { "index.html" } else { path };
    if requested_path.contains("..") {
        return None;
    }

    if matches!(requested_path, "docs/help.css" | "help.css") {
        return Some("help.css".to_string());
    }

    Some(format!("help/{requested_path}"))
}

fn dist_asset_response(path: &str) -> Option<Response> {
    embedded_asset_response::<DistAssets>(path)
}

fn docs_asset_response(path: String) -> Option<Response> {
    embedded_asset_response::<DocsAssets>(&path)
}

fn embedded_asset_response<T: RustEmbed>(path: &str) -> Option<Response> {
    let asset = T::get(path)?;
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime.as_ref())
        .body(Body::from(asset.data.into_owned()))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::help_asset_key;

    #[test]
    fn help_asset_key_defaults_to_help_index() {
        assert_eq!(help_asset_key(""), Some("help/index.html".to_string()));
    }

    #[test]
    fn help_asset_key_maps_help_css_alias() {
        assert_eq!(
            help_asset_key("docs/help.css"),
            Some("help.css".to_string())
        );
        assert_eq!(help_asset_key("help.css"), Some("help.css".to_string()));
    }

    #[test]
    fn help_asset_key_rejects_path_traversal() {
        assert_eq!(help_asset_key("../secret"), None);
    }
}
