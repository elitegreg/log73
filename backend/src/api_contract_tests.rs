use super::*;
use crate::db::set_contact_adif;
use crate::tests::{cache_test_contact, cache_test_state};
use axum::{body::to_bytes, http::Request};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn request(app: &Router, method: &str, path: &str, payload: Option<Value>) -> Response {
    let mut request = Request::builder().method(method).uri(path);
    let body = if let Some(payload) = payload {
        request = request.header(header::CONTENT_TYPE, "application/json");
        Body::from(payload.to_string())
    } else {
        Body::empty()
    };
    app.clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap()
}

async fn json_response(response: Response, status: StatusCode) -> Value {
    let actual_status = response.status();
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(actual_status, status, "{value}");
    assert!(value.get("ok").is_none(), "legacy envelope: {value}");
    if status.is_client_error() || status.is_server_error() {
        assert!(value["error"].is_string(), "missing error message: {value}");
    }
    value
}

async fn test_log(state: &AppState, category: &str) -> db::Log {
    state
        .db
        .create_log(NewLog {
            name: "API contract".into(),
            contest_id: "CQ-WPX-CW".into(),
            station_callsign: "N0CALL".into(),
            contest_params: json!({"CATEGORY-TRANSMITTER": category}),
        })
        .await
        .unwrap()
}

fn valid_contact(call: &str) -> Contact {
    let mut contact = cache_test_contact(call);
    set_contact_adif(&mut contact, "CONTEST_ID", json!("CQ-WPX-CW"));
    set_contact_adif(&mut contact, "STX", json!(1));
    set_contact_adif(&mut contact, "SRX", json!(1));
    contact
}

#[tokio::test]
async fn message_defaults_and_labels_are_direct_values_and_invalid_text_is_400() {
    let state = cache_test_state();
    let app = app_router(state.clone(), state.radio_io.clone());
    for (kind, field) in [
        ("cw", "cw_messages"),
        ("digital", "digital_messages"),
        ("voice", "voice_messages"),
    ] {
        let base = format!("/api/radios/{kind}-messages");
        let defaults = json_response(
            request(&app, "GET", &format!("{base}/default"), None).await,
            StatusCode::OK,
        )
        .await;
        assert!(defaults.is_string());
        let labels = json_response(
            request(
                &app,
                "POST",
                &format!("{base}/validate"),
                Some(json!({field: defaults})),
            )
            .await,
            StatusCode::OK,
        )
        .await;
        assert!(labels["run"].is_array());
        assert!(labels["s&p"].is_array());
        json_response(
            request(
                &app,
                "POST",
                &format!("{base}/validate"),
                Some(json!({field: ""})),
            )
            .await,
            StatusCode::BAD_REQUEST,
        )
        .await;
    }
}

#[tokio::test]
async fn radio_crud_preserves_direct_objects_and_conflict_status() {
    let state = cache_test_state();
    let app = app_router(state.clone(), state.radio_io.clone());
    let mut payload = json!({"name": "Dummy", "radio_kind": "dummy", "transport_kind": "none"});
    let radio = json_response(
        request(&app, "POST", "/api/radios", Some(payload.clone())).await,
        StatusCode::OK,
    )
    .await;
    let id = radio["id"].as_i64().unwrap();
    assert_eq!(radio["name"], "Dummy");
    assert!(radio["radio_ws_url"].is_string());
    let path = format!("/api/radios/{id}");
    assert_eq!(
        json_response(request(&app, "GET", &path, None).await, StatusCode::OK).await,
        radio
    );
    for suffix in ["cw-labels", "message-labels"] {
        assert!(
            json_response(
                request(&app, "GET", &format!("{path}/{suffix}"), None).await,
                StatusCode::OK
            )
            .await
            .is_object()
        );
    }
    let mutation = state.radio_io.begin_mutation(id).unwrap();
    json_response(
        request(&app, "PUT", &path, Some(payload.clone())).await,
        StatusCode::CONFLICT,
    )
    .await;
    drop(mutation);
    payload["name"] = json!("Updated");
    assert_eq!(
        json_response(
            request(&app, "PUT", &path, Some(payload)).await,
            StatusCode::OK
        )
        .await["name"],
        "Updated"
    );
    assert_eq!(
        json_response(request(&app, "DELETE", &path, None).await, StatusCode::OK).await,
        true
    );
    json_response(
        request(&app, "DELETE", &path, None).await,
        StatusCode::NOT_FOUND,
    )
    .await;
}

#[tokio::test]
async fn radio_validation_preserves_asset_checks_and_internal_error_provenance() {
    let state = cache_test_state();
    let app = app_router(state.clone(), state.radio_io.clone());
    json_response(
        request(
            &app,
            "POST",
            "/api/radios",
            Some(json!({"name": "Invalid"})),
        )
        .await,
        StatusCode::BAD_REQUEST,
    )
    .await;
    // Standalone voice syntax validation does not require assets; radio configuration does.
    let voice = "# RUN Messages\nF1 CQ,missing.wav\n# S&P Messages\nF1 CQ,missing.wav";
    json_response(
        request(
            &app,
            "POST",
            "/api/radios/voice-messages/validate",
            Some(json!({"voice_messages": voice})),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let payload = json!({"name": "Dummy", "radio_kind": "dummy", "transport_kind": "none", "voice_messages": voice});
    json_response(
        request(&app, "POST", "/api/radios", Some(payload)).await,
        StatusCode::BAD_REQUEST,
    )
    .await;
    state.blocking_work.permits().close();
    json_response(
        request(
            &app,
            "POST",
            "/api/radios",
            Some(json!({"name": "Dummy", "radio_kind": "dummy", "transport_kind": "none"})),
        )
        .await,
        StatusCode::INTERNAL_SERVER_ERROR,
    )
    .await;
}

#[tokio::test]
async fn single_and_batch_commits_keep_both_contact_fields_and_stats_are_direct() {
    let state = cache_test_state();
    let log = test_log(&state, "ONE").await;
    let app = app_router(state.clone(), state.radio_io.clone());
    let path = format!("/api/logs/{}/contacts", log.id);
    for payload in [
        json!(valid_contact("K1ABC")),
        json!([valid_contact("K2ABC"), valid_contact("K3ABC")]),
    ] {
        let expected = payload.as_array().map_or(1, Vec::len);
        let result = json_response(
            request(&app, "POST", &path, Some(payload)).await,
            StatusCode::OK,
        )
        .await;
        assert_eq!(result["contacts"].as_array().unwrap().len(), expected);
        assert_eq!(result["contact"], result["contacts"][0]);
        assert_eq!(result["contact"]["meta"]["status"], "Committed");
    }
    let contacts = json_response(request(&app, "GET", &path, None).await, StatusCode::OK).await;
    assert_eq!(contacts.as_array().unwrap().len(), 3);
    let stats = json_response(
        request(&app, "GET", &format!("/api/logs/{}/stats", log.id), None).await,
        StatusCode::OK,
    )
    .await;
    assert!(stats["overall"].is_object());
    let id = contacts[0]["meta"]["id"].as_i64().unwrap();
    let path = format!("/api/contacts/{id}");
    assert_eq!(
        json_response(request(&app, "DELETE", &path, None).await, StatusCode::OK).await,
        true
    );
    json_response(
        request(&app, "DELETE", &path, None).await,
        StatusCode::NOT_FOUND,
    )
    .await;
}

#[tokio::test]
async fn warnings_are_successful_and_integrity_and_write_failures_are_http_errors() {
    let state = cache_test_state();
    let log = test_log(&state, "ONE").await;
    let app = app_router(state.clone(), state.radio_io.clone());
    let path = format!("/api/logs/{}/contacts", log.id);
    let mut contact = valid_contact("K1ABC");
    contact
        .get_mut("adif")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("SRX");
    let error = json_response(
        request(&app, "POST", &path, Some(json!(contact.clone()))).await,
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(error["status"], "failed");
    set_contact_meta(&mut contact, "force", json!(true));
    set_contact_meta(&mut contact, "source", json!("wsjtx"));
    set_contact_meta(&mut contact, "clientId", json!(Uuid::new_v4().to_string()));
    let result = json_response(
        request(&app, "POST", &path, Some(json!(contact))).await,
        StatusCode::OK,
    )
    .await;
    assert!(
        !result["contact"]["meta"]["warnings"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let mut edit = result["contact"].clone();
    edit["adif"]["STX"] = json!(2);
    json_response(
        request(&app, "POST", &path, Some(edit)).await,
        StatusCode::BAD_REQUEST,
    )
    .await;
    let other_log = test_log(&state, "ONE").await;
    json_response(
        request(
            &app,
            "POST",
            &format!("/api/logs/{}/contacts", other_log.id),
            Some(result["contact"].clone()),
        )
        .await,
        StatusCode::BAD_REQUEST,
    )
    .await;
    state.db.test_control(|connection| connection.execute_batch("CREATE TRIGGER fail_insert BEFORE INSERT ON qsos BEGIN SELECT RAISE(ABORT, 'forced database failure'); END")).await.unwrap();
    json_response(
        request(&app, "POST", &path, Some(json!(valid_contact("K4ABC")))).await,
        StatusCode::INTERNAL_SERVER_ERROR,
    )
    .await;
}

#[tokio::test]
async fn serial_state_and_reservations_are_direct_and_overflow_is_internal() {
    let state = cache_test_state();
    let log = state
        .db
        .create_log(NewLog {
            name: "Serial reservations".into(),
            contest_id: "RSGB-IOTA (Island)".into(),
            station_callsign: "N0CALL".into(),
            contest_params: json!({"CATEGORY-TRANSMITTER": "TWO"}),
        })
        .await
        .unwrap();
    let app = app_router(state.clone(), state.radio_io.clone());
    let path = format!("/api/logs/{}/serial-allocation", log.id);
    let serial = json_response(
        request(&app, "GET", &format!("{path}?field_adif=STX"), None).await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(serial["next"], 1);
    assert_eq!(serial["reservation_required"], true);
    let reserved = json_response(
        request(&app, "POST", &path, Some(json!({"field_adif": "STX"}))).await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(reserved["serial"], 1);
    assert_eq!(reserved["log_id"], log.id);
    json_response(
        request(&app, "POST", &path, Some(json!({"field_adif": "CALL"}))).await,
        StatusCode::BAD_REQUEST,
    )
    .await;
    let mut contact = valid_contact("K1ABC");
    set_contact_adif(&mut contact, "STX", json!(i64::MAX));
    state
        .db
        .upsert_contacts(log.id, vec![contact])
        .await
        .unwrap();
    json_response(
        request(&app, "GET", &format!("{path}?field_adif=STX"), None).await,
        StatusCode::INTERNAL_SERVER_ERROR,
    )
    .await;
    json_response(
        request(&app, "POST", &path, Some(json!({"field_adif": "STX"}))).await,
        StatusCode::INTERNAL_SERVER_ERROR,
    )
    .await;
}

#[tokio::test]
async fn imports_keep_record_diagnostics_and_downloads_keep_filenames() {
    let state = cache_test_state();
    let log = test_log(&state, "ONE").await;
    let app = app_router(state.clone(), state.radio_io.clone());
    let path = format!("/api/logs/{}/adif/import", log.id);
    let error = json_response(
        request(
            &app,
            "POST",
            &path,
            Some(json!({"adif": "<CALL:5>K1ABC<EOR>"})),
        )
        .await,
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert!(error["line"].is_number());
    assert_eq!(error["errors"][0]["line"], error["line"]);
    let text = adif::render_log(&log, &[valid_contact("K1ABC")]).unwrap();
    let mappings = state
        .contest_rules
        .get(&log.contest_id)
        .unwrap()
        .exchange
        .iter()
        .map(|field| {
            (
                field.adif.clone(),
                json!({"kind": "adif_field", "field": field.adif}),
            )
        })
        .collect::<serde_json::Map<String, Value>>();
    let imported = json_response(
        request(
            &app,
            "POST",
            &path,
            Some(json!({"adif": text, "mappings": mappings})),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(imported["imported"], 1);
    assert_eq!(imported["contacts"].as_array().unwrap().len(), 1);
    let response = request(&app, "POST", &format!("/api/logs/{}/adif", log.id), None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_DISPOSITION],
        "attachment; filename=\"N0CALL.adi\""
    );
    let text = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert!(String::from_utf8_lossy(&text).contains("K1ABC"));
}

#[tokio::test]
async fn missing_resources_and_extractor_errors_share_json_error_contract() {
    let state = cache_test_state();
    let app = app_router(state.clone(), state.radio_io.clone());
    for (method, path, payload) in [
        ("GET", "/api/logs/999/stats", None),
        ("GET", "/api/logs/999/contacts", None),
        (
            "GET",
            "/api/logs/999/serial-allocation?field_adif=STX",
            None,
        ),
        (
            "POST",
            "/api/logs/999/contacts",
            Some(json!(valid_contact("K1ABC"))),
        ),
        ("POST", "/api/logs/999/adif/import", Some(json!({}))),
        ("POST", "/api/logs/999/adif", None),
        ("POST", "/api/logs/999/cabrillo", Some(json!({}))),
        ("DELETE", "/api/logs/999", None),
    ] {
        json_response(
            request(&app, method, path, payload).await,
            StatusCode::NOT_FOUND,
        )
        .await;
    }
    for (method, path, body) in [
        ("POST", "/api/radios/cw-messages/validate", "{broken"),
        ("POST", "/api/radios/cw-messages/validate", "{}"),
        ("GET", "/api/radios/not-an-id", ""),
        ("GET", "/api/logs/1/contacts?limit=broken", ""),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        json_response(response, StatusCode::BAD_REQUEST).await;
    }
}

#[tokio::test]
async fn radio_client_registration_keeps_wire_fields_and_lease_acknowledgements_are_204() {
    let state = cache_test_state();
    let app = app_router(state.clone(), state.radio_io.clone());
    let path = format!("/api/radio-clients/{}", Uuid::new_v4());
    let registration = json_response(
        request(
            &app,
            "PUT",
            &path,
            Some(json!({
                "radio_ws_url": "ws://127.0.0.1:49152/radiows",
                "config": {"name": "Remote", "radio_kind": "dummy", "transport_kind": "none"}
            })),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert!(registration["heartbeat_interval_seconds"].as_u64().unwrap() > 0);
    let lease = json!({"radio_id": registration["radio_id"], "lease_id": registration["lease_id"]});
    for operation in ["heartbeat", "offline"] {
        let response = request(
            &app,
            "POST",
            &format!("{path}/{operation}"),
            Some(lease.clone()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .is_empty()
        );
    }
    json_response(
        request(&app, "POST", &format!("{path}/heartbeat"), Some(lease)).await,
        StatusCode::CONFLICT,
    )
    .await;
}
