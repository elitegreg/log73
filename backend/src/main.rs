mod bands;
mod frequency;
mod scqso_in_state;

use axum::{Json, Router, routing::get};
use scqso_in_state::ContestRules;
use tower_http::cors::CorsLayer;

type Contact = serde_json::Map<String, serde_json::Value>;

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/contest-settings/get", get(contest_settings))
        .route("/contacts/get", get(contacts))
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
        .await
        .expect("failed to bind backend to 0.0.0.0:8080");

    println!("log73 backend listening on http://0.0.0.0:8080");
    axum::serve(listener, app).await.expect("server failed");
}

async fn contest_settings() -> Json<ContestRules> {
    Json(ContestRules::new())
}

async fn contacts() -> Json<Vec<Contact>> {
    Json(Vec::new())
}
