//! `PUT /settings`: what an edit stores, what it refuses, and the page it
//! answers.

use axum::http::StatusCode;
use pdo_sandbox::domain::retired_settings::{
    retired_settings_warning, RETIRED_SANDBOX_IMAGE_COLUMNS,
};
use pdo_settings::adapters::sqlite::{agent_profiles, instance_config, SqliteInstanceConfig};
use pdo_settings::application::boot::warn_about_retired_settings;
use pdo_settings::domain::agent_profile::ProfileDraft;

use super::{get_settings_json, put_settings_resp, test_state};

#[tokio::test]
async fn put_settings_round_trips_the_autocomplete_flag_both_ways() {
    // #469 §4: the checkbox is authoritative in BOTH directions. Unticking must
    // persist a stored `0` (source still "stored"), not clear back to unset —
    // otherwise it could not override `PDO_AUTOCOMPLETE_TURN_END=1`.
    let state = test_state().await;

    let (status, view) = put_settings_resp(&state, r#"{"autocomplete_turn_end": true}"#).await;
    assert_eq!(status, StatusCode::OK, "got {view}");
    assert_eq!(view["autocomplete_turn_end"]["effective"], true);
    assert_eq!(view["autocomplete_turn_end"]["source"], "stored");
    assert_eq!(
        instance_config::get(&state.db)
            .await
            .unwrap()
            .autocomplete_turn_end,
        Some(1)
    );

    let (status, view) = put_settings_resp(&state, r#"{"autocomplete_turn_end": false}"#).await;
    assert_eq!(status, StatusCode::OK, "got {view}");
    assert_eq!(view["autocomplete_turn_end"]["effective"], false);
    assert_eq!(
        view["autocomplete_turn_end"]["source"], "stored",
        "unticking is a stored decision, not a fall-through: {view}"
    );
    assert_eq!(
        instance_config::get(&state.db)
            .await
            .unwrap()
            .autocomplete_turn_end,
        Some(0),
        "off must persist a stored 0, never NULL"
    );
}

#[tokio::test]
async fn put_settings_round_trips_the_default_auto_name_flag_both_ways() {
    // The checkbox is authoritative in BOTH directions. Unticking must persist a
    // stored `0` (source still "stored"), not clear back to unset — otherwise it could
    // not override `PDO_DEFAULT_AUTO_NAME=1`.
    let state = test_state().await;

    let (status, view) = put_settings_resp(&state, r#"{"default_auto_name": false}"#).await;
    assert_eq!(status, StatusCode::OK, "got {view}");
    assert_eq!(view["default_auto_name"]["effective"], false);
    assert_eq!(view["default_auto_name"]["source"], "stored");
    assert_eq!(
        instance_config::get(&state.db)
            .await
            .unwrap()
            .default_auto_name,
        Some(0),
        "off must persist a stored 0, never NULL"
    );

    let (status, view) = put_settings_resp(&state, r#"{"default_auto_name": true}"#).await;
    assert_eq!(status, StatusCode::OK, "got {view}");
    assert_eq!(view["default_auto_name"]["effective"], true);
    assert_eq!(view["default_auto_name"]["source"], "stored");
    assert_eq!(
        instance_config::get(&state.db)
            .await
            .unwrap()
            .default_auto_name,
        Some(1)
    );
}

#[tokio::test]
async fn put_settings_round_trips_the_review_agent_can_resolve_flag_both_ways() {
    // #751: default OFF (the human resolves); both directions of a save are a
    // stored decision — unticking persists a `0` that beats
    // `PDO_REVIEW_AGENT_CAN_RESOLVE=1`.
    let state = test_state().await;
    let fresh = get_settings_json(&state).await;
    assert_eq!(fresh["review_agent_can_resolve"]["default"], false);
    assert!(fresh["review_agent_can_resolve"]["stored"].is_null());
    if fresh["review_agent_can_resolve"]["env"].is_null() {
        assert_eq!(fresh["review_agent_can_resolve"]["effective"], false);
        assert_eq!(fresh["review_agent_can_resolve"]["source"], "default");
    }

    let (status, view) = put_settings_resp(&state, r#"{"review_agent_can_resolve": true}"#).await;
    assert_eq!(status, StatusCode::OK, "got {view}");
    assert_eq!(view["review_agent_can_resolve"]["effective"], true);
    assert_eq!(view["review_agent_can_resolve"]["source"], "stored");
    assert_eq!(
        instance_config::get(&state.db)
            .await
            .unwrap()
            .review_agent_can_resolve,
        Some(1)
    );

    let (status, view) = put_settings_resp(&state, r#"{"review_agent_can_resolve": false}"#).await;
    assert_eq!(status, StatusCode::OK, "got {view}");
    assert_eq!(view["review_agent_can_resolve"]["effective"], false);
    assert_eq!(view["review_agent_can_resolve"]["source"], "stored");
    assert_eq!(
        instance_config::get(&state.db)
            .await
            .unwrap()
            .review_agent_can_resolve,
        Some(0),
        "off must persist a stored 0, never NULL"
    );
}

#[tokio::test]
async fn put_settings_round_trips_the_manager_enabled_flag_both_ways() {
    // Manager on demand: the checkbox is authoritative in BOTH directions.
    // Unticking must persist a stored `0` (not a clear back to NULL), or it
    // could never override a `PDO_MANAGER_ENABLED=1`. The fresh view also
    // discloses the built-in default: OFF — a Run starts managerless.
    let state = test_state().await;

    let (_, fresh) = put_settings_resp(&state, "{}").await;
    assert_eq!(fresh["manager_enabled"]["effective"], false);
    assert_eq!(fresh["manager_enabled"]["source"], "default");
    assert_eq!(fresh["manager_enabled"]["default"], false);
    assert_eq!(
        fresh["manager_profile"]["effective"],
        serde_json::Value::Null
    );

    let (status, view) = put_settings_resp(&state, r#"{"manager_enabled": true}"#).await;
    assert_eq!(status, StatusCode::OK, "got {view}");
    assert_eq!(view["manager_enabled"]["effective"], true);
    assert_eq!(view["manager_enabled"]["source"], "stored");
    assert_eq!(
        instance_config::get(&state.db)
            .await
            .unwrap()
            .manager_enabled,
        Some(1),
        "on must persist a stored 1"
    );

    let (status, view) = put_settings_resp(&state, r#"{"manager_enabled": false}"#).await;
    assert_eq!(status, StatusCode::OK, "got {view}");
    assert_eq!(view["manager_enabled"]["effective"], false);
    assert_eq!(view["manager_enabled"]["source"], "stored");
    assert_eq!(
        instance_config::get(&state.db)
            .await
            .unwrap()
            .manager_enabled,
        Some(0),
        "off must persist a stored 0, never NULL"
    );
}

#[tokio::test]
async fn put_settings_persists_and_clears_the_manager_profile_pin() {
    let state = test_state().await;
    let profile = agent_profiles::create(
        &state.db,
        ProfileDraft::new("reviewer", "claude", Some("sonnet"), None).unwrap(),
    )
    .await
    .unwrap();

    let (status, view) = put_settings_resp(&state, r#"{"manager_profile": "reviewer"}"#).await;
    assert_eq!(status, StatusCode::OK, "got {view}");
    assert_eq!(view["manager_profile"]["effective"], "reviewer");
    assert_eq!(view["manager_profile"]["source"], "stored");

    // "" is the clear sentinel — back to « Follow the Run », persisted as
    // SQL NULL, never as an empty string that would win precedence.
    let (status, view) = put_settings_resp(&state, r#"{"manager_profile": ""}"#).await;
    assert_eq!(status, StatusCode::OK, "got {view}");
    assert_eq!(
        view["manager_profile"]["effective"],
        serde_json::Value::Null
    );
    assert_eq!(view["manager_profile"]["source"], "default");
    assert_eq!(
        instance_config::get(&state.db)
            .await
            .unwrap()
            .manager_profile,
        None,
        "clear must persist NULL, never ''"
    );

    // Keep the profile referenced so `create`'s row is not dead code.
    assert_eq!(profile.name, "reviewer");
}

#[tokio::test]
async fn put_settings_refuses_an_unknown_manager_profile() {
    // A pin to nothing would silently fall back to « Follow the Run » at
    // spawn time — refused at registration instead. (The #432 tombstone
    // only ever arises from a LATER profile deletion, which no PUT can
    // prevent.)
    let state = test_state().await;
    let (status, body) = put_settings_resp(&state, r#"{"manager_profile": "ghost"}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "got {body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("unknown manager profile `ghost`"),
        "got {body}"
    );
    assert_eq!(
        instance_config::get(&state.db)
            .await
            .unwrap()
            .manager_profile,
        None,
        "a refused edit must persist nothing"
    );
}

#[tokio::test]
async fn put_settings_persists_and_stored_wins() {
    // Stored always wins over env/default, so these assertions are race-free.
    let state = test_state().await;
    let (status, view) = put_settings_resp(
        &state,
        r#"{"session_cap": 4, "reaper_ttl_secs": 120, "guard_timeout_secs": 30}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // PUT returns the recomputed view (D6) — no second round-trip needed.
    assert_eq!(view["session_cap"]["stored"], 4);
    assert_eq!(view["session_cap"]["source"], "stored");
    assert_eq!(view["session_cap"]["effective"], 4);
    assert_eq!(view["reaper_ttl_secs"]["stored"], 120);
    assert_eq!(view["reaper_ttl_secs"]["source"], "stored");
    assert_eq!(view["reaper_ttl_secs"]["effective"], 120);
    assert_eq!(view["guard_timeout_secs"]["stored"], 30);
    assert_eq!(view["guard_timeout_secs"]["source"], "stored");
    assert_eq!(view["guard_timeout_secs"]["effective"], 30);

    // Re-GET confirms persistence.
    let reget = get_settings_json(&state).await;
    assert_eq!(reget["session_cap"]["stored"], 4);
    assert_eq!(reget["session_cap"]["effective"], 4);
}

#[tokio::test]
async fn put_settings_persists_default_model() {
    // Stored always wins over env/default, so the effective/source assertions
    // are race-free even with a concurrent env-mutating test.
    let state = test_state().await;
    let (status, view) = put_settings_resp(&state, r#"{"default_model": "opus"}"#).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["default_model"]["stored"], "opus");
    assert_eq!(view["default_model"]["source"], "stored");
    assert_eq!(view["default_model"]["effective"], "opus");
    assert!(view["default_model"]["default"].is_null());

    // Re-GET confirms persistence.
    let reget = get_settings_json(&state).await;
    assert_eq!(reget["default_model"]["stored"], "opus");
    assert_eq!(reget["default_model"]["effective"], "opus");
}

#[tokio::test]
async fn put_settings_clears_default_model_on_empty_string() {
    // "" is the clear sentinel (D): it must NOT be rejected, and it must reset
    // the stored tier to null so the resolver falls through to env/account.
    let state = test_state().await;
    put_settings_resp(&state, r#"{"default_model": "opus"}"#).await;
    let (status, view) = put_settings_resp(&state, r#"{"default_model": ""}"#).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "empty string must be accepted, not 400"
    );
    assert!(
        view["default_model"]["stored"].is_null(),
        "empty string clears the stored model: {}",
        view["default_model"]
    );
    assert_eq!(view["default_model"]["source"], "default");
}

#[tokio::test]
async fn put_settings_rejects_cap_below_one() {
    let state = test_state().await;
    let (status, body) = put_settings_resp(&state, r#"{"session_cap": 0}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].is_string(), "400 must carry an error: {body}");
    // Store untouched.
    let view = get_settings_json(&state).await;
    assert!(view["session_cap"]["stored"].is_null());
}

#[tokio::test]
async fn put_settings_rejects_ttl_below_one() {
    let state = test_state().await;
    let (status, _) = put_settings_resp(&state, r#"{"reaper_ttl_secs": 0}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let view = get_settings_json(&state).await;
    assert!(view["reaper_ttl_secs"]["stored"].is_null());
}

#[tokio::test]
async fn put_settings_rejects_guard_out_of_range() {
    let state = test_state().await;
    let (lo, _) = put_settings_resp(&state, r#"{"guard_timeout_secs": 0}"#).await;
    assert_eq!(lo, StatusCode::BAD_REQUEST);
    let (hi, _) = put_settings_resp(&state, r#"{"guard_timeout_secs": 601}"#).await;
    assert_eq!(hi, StatusCode::BAD_REQUEST);
    // A boundary value is accepted.
    let (ok, view) = put_settings_resp(&state, r#"{"guard_timeout_secs": 600}"#).await;
    assert_eq!(ok, StatusCode::OK);
    assert_eq!(view["guard_timeout_secs"]["stored"], 600);
}

#[tokio::test]
async fn put_settings_partial_edit_leaves_other_fields() {
    let state = test_state().await;
    put_settings_resp(&state, r#"{"session_cap": 7}"#).await;
    let (_, view) = put_settings_resp(&state, r#"{"reaper_ttl_secs": 200}"#).await;
    // The second edit must not clear the cap set by the first.
    assert_eq!(view["session_cap"]["stored"], 7);
    assert_eq!(view["reaper_ttl_secs"]["stored"], 200);
}

/// AC2: a `PUT` carrying a retired field is a **400 naming the field**, not a 200 that
/// silently ignores it. Both fields, and the store is untouched either way.
#[tokio::test]
async fn put_settings_rejects_a_retired_sandbox_image_field() {
    let state = test_state().await;
    for (field, env_var) in RETIRED_SANDBOX_IMAGE_COLUMNS {
        let (status, body) =
            put_settings_resp(&state, &format!(r#"{{"{field}": "registry"}}"#)).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "`{field}` must be refused, not ignored: {body}"
        );
        let err = body["error"].as_str().unwrap_or_default();
        assert!(err.contains(field), "the 400 must NAME the field: {body}");
        assert!(
            err.contains("staging profile") && err.contains(env_var),
            "…and say where the knob went: {body}"
        );
    }
    // Nothing was persisted.
    let view = get_settings_json(&state).await;
    assert!(view["session_cap"]["stored"].is_null());
}

/// The refusal must not let a legitimate edit riding in the same body through: a `PUT` is one
/// transaction, so a retired field poisons the whole thing rather than applying half of it.
#[tokio::test]
async fn a_retired_field_rejects_the_whole_put() {
    let state = test_state().await;
    let (status, _) = put_settings_resp(
        &state,
        r#"{"session_cap": 5, "image_source": "dockerfile"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let view = get_settings_json(&state).await;
    assert!(
        view["session_cap"]["stored"].is_null(),
        "the sibling edit must NOT have been applied: {view}"
    );
}

/// A field nobody has ever heard of stays what it always was — ignored by serde. Only the
/// two RETIRED names get the named 400: they are the ones a real client might still send.
#[tokio::test]
async fn an_unknown_field_is_still_ignored_not_rejected() {
    let state = test_state().await;
    let (status, _) = put_settings_resp(&state, r#"{"session_cap": 5, "no_such_knob": 1}"#).await;
    assert_eq!(status, StatusCode::OK);
    let view = get_settings_json(&state).await;
    assert_eq!(view["session_cap"]["stored"], 5);
}

/// A malformed body is a 400 with a message, not axum's opaque 422 — the consequence of
/// taking the body as raw JSON so the retired-field check can run first.
#[tokio::test]
async fn a_type_error_in_the_body_is_a_400_with_a_message() {
    let state = test_state().await;
    let (status, body) = put_settings_resp(&state, r#"{"session_cap": "many"}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("invalid settings payload"),
        "{body}"
    );
}

/// AC5, the wiring half: the boot path reads a seeded value out of a pre-#471 row and produces
/// exactly that message. Exercises `retired_sandbox_values` + the profile hint together,
/// which is the pair the boot uses — `warn_about_retired_settings` itself only logs.
#[tokio::test]
async fn a_seeded_retired_value_produces_the_boot_warning() {
    let state = test_state().await;
    // Re-add the columns a pre-#471 daemon left behind, and seed one.
    for (column, _) in RETIRED_SANDBOX_IMAGE_COLUMNS {
        sqlx::query(&format!(
            "ALTER TABLE instance_config ADD COLUMN {column} TEXT"
        ))
        .execute(&state.db)
        .await
        .unwrap();
    }
    sqlx::query("UPDATE instance_config SET image_source = 'dockerfile' WHERE id = 1")
        .execute(&state.db)
        .await
        .unwrap();
    put_settings_resp(&state, r#"{"default_sandbox": "minimal"}"#).await;

    let stored = instance_config::retired_sandbox_values(&state.db)
        .await
        .unwrap();
    assert_eq!(stored, vec![("image_source", "dockerfile".to_string())]);
    let msg = retired_settings_warning(&stored, Some("minimal")).unwrap();
    assert!(msg.contains("`image_source` = `dockerfile`"), "{msg}");
    assert!(msg.contains("`minimal`"), "{msg}");
    // And it did NOT come back as config: the view has no such key, so no Run can read it.
    let view = get_settings_json(&state).await;
    assert!(view.get("image_source").is_none(), "{view}");
    // The logging wrapper is exercised too — it must not panic on this row.
    warn_about_retired_settings(&SqliteInstanceConfig::new(state.db.clone())).await;
}

#[tokio::test]
async fn put_settings_persists_default_sandbox() {
    // Stored always wins over env/default, so the effective/source assertions are
    // race-free (the `lib.rs` convention: env reads a process-global a concurrent test
    // could shadow, so only stored-wins is safe at the HTTP level).
    let state = test_state().await;
    let (status, view) = put_settings_resp(&state, r#"{"default_sandbox": "minimal"}"#).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["default_sandbox"]["stored"], "minimal");
    assert_eq!(view["default_sandbox"]["source"], "stored");
    assert_eq!(view["default_sandbox"]["effective"], "minimal");
    assert_eq!(view["default_sandbox"]["default"], "off");

    // Re-GET confirms persistence.
    let reget = get_settings_json(&state).await;
    assert_eq!(reget["default_sandbox"]["stored"], "minimal");
    assert_eq!(reget["default_sandbox"]["effective"], "minimal");
}

#[tokio::test]
async fn put_settings_clears_default_sandbox_on_empty_string() {
    // "" is the clear sentinel: accepted (not 400), resets the stored tier to null
    // so the resolver falls back to the built-in default `off`.
    let state = test_state().await;
    put_settings_resp(&state, r#"{"default_sandbox": "minimal"}"#).await;
    let (status, view) = put_settings_resp(&state, r#"{"default_sandbox": ""}"#).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "empty string must be accepted, not 400"
    );
    assert!(
        view["default_sandbox"]["stored"].is_null(),
        "empty string clears the stored default sandbox: {}",
        view["default_sandbox"]
    );
    assert_eq!(view["default_sandbox"]["source"], "default");
    assert_eq!(view["default_sandbox"]["effective"], "off");
}

#[tokio::test]
async fn put_settings_rejects_invalid_default_sandbox() {
    // A non-variant token is rejected 400 before anything is persisted.
    let state = test_state().await;
    let (status, body) = put_settings_resp(&state, r#"{"default_sandbox": "vm"}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].is_string(), "400 must carry an error: {body}");
    let view = get_settings_json(&state).await;
    assert!(view["default_sandbox"]["stored"].is_null());
}
