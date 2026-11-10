//! The OTLP export on the settings page (ADR-0090): off by default, its
//! collector validated, its headers stored but never shown again.

use axum::http::StatusCode;
use pdo_settings::adapters::sqlite::instance_config;

use super::{get_settings_json, put_settings_resp, test_state};

#[tokio::test]
async fn the_export_is_off_by_default_with_no_collector_and_no_header() {
    let state = test_state().await;
    let page = get_settings_json(&state).await;
    assert_eq!(page["telemetry_otlp"]["effective"], false);
    assert_eq!(page["telemetry_otlp"]["source"], "default");
    assert!(page["telemetry_otlp_endpoint"]["effective"].is_null());
    assert_eq!(page["telemetry_otlp_headers"], serde_json::json!([]));
}

#[tokio::test]
async fn an_edit_stores_the_switch_the_collector_and_the_headers() {
    let state = test_state().await;
    let (status, page) = put_settings_resp(
        &state,
        r#"{"telemetry_otlp": true,
            "telemetry_otlp_endpoint": "https://otel.example.org:4318",
            "telemetry_otlp_headers": [{"name": "Authorization", "value": "Bearer s3cret"}]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "got {page}");
    assert_eq!(page["telemetry_otlp"]["effective"], true);
    assert_eq!(page["telemetry_otlp"]["source"], "stored");
    assert_eq!(
        page["telemetry_otlp_endpoint"]["effective"],
        "https://otel.example.org:4318"
    );
    assert_eq!(
        page["telemetry_otlp_headers"],
        serde_json::json!([{ "name": "Authorization", "set": true }])
    );
    assert!(
        !page.to_string().contains("s3cret"),
        "the page never carries a header's value: {page}"
    );
    let stored = instance_config::get(&state.db).await.unwrap();
    assert_eq!(stored.telemetry_otlp, Some(1));
    assert_eq!(
        stored
            .telemetry_otlp_headers
            .get("Authorization")
            .map(String::as_str),
        Some("Bearer s3cret")
    );
}

#[tokio::test]
async fn re_saving_a_header_without_its_value_keeps_it_and_omitting_it_removes_it() {
    let state = test_state().await;
    put_settings_resp(
        &state,
        r#"{"telemetry_otlp_headers": [{"name": "Authorization", "value": "Bearer s3cret"}]}"#,
    )
    .await;
    let (status, _) = put_settings_resp(
        &state,
        r#"{"telemetry_otlp_headers": [{"name": "Authorization", "value": null},
                                       {"name": "X-Team", "value": "core"}]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let stored = instance_config::get(&state.db).await.unwrap();
    assert_eq!(
        stored
            .telemetry_otlp_headers
            .get("Authorization")
            .map(String::as_str),
        Some("Bearer s3cret")
    );
    assert_eq!(stored.telemetry_otlp_headers.len(), 2);

    put_settings_resp(&state, r#"{"telemetry_otlp_headers": []}"#).await;
    let stored = instance_config::get(&state.db).await.unwrap();
    assert!(stored.telemetry_otlp_headers.is_empty());
}

#[tokio::test]
async fn a_collector_that_is_not_an_http_address_is_refused_and_nothing_is_stored() {
    let state = test_state().await;
    let (status, body) = put_settings_resp(
        &state,
        r#"{"telemetry_otlp": true, "telemetry_otlp_endpoint": "otel.example.org:4318"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "got {body}");
    assert!(body["error"].as_str().unwrap().contains("http"));
    let stored = instance_config::get(&state.db).await.unwrap();
    assert_eq!(stored.telemetry_otlp, None, "a refused edit stores nothing");
}

#[tokio::test]
async fn a_new_header_without_a_value_is_refused() {
    let state = test_state().await;
    let (status, _) = put_settings_resp(
        &state,
        r#"{"telemetry_otlp_headers": [{"name": "Authorization", "value": null}]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
