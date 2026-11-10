//! `GET /settings`: the page's shape, each section's tiers; `POST
//! /settings/harnesses/{name}/refresh`: one harness probed again.

use pdo_settings::adapters::sqlite::instance_config;
use pdo_settings::domain::instance_config::UpdateInstanceConfig;

use axum::http::StatusCode;

use super::{get_settings_json, post_json, test_state};

#[tokio::test]
async fn settings_view_discloses_the_autocomplete_tiers() {
    // #469 §4 / ADR-0015: `effective` comes from the SAME resolver the sweep
    // consumes, and both directions of an explicit save are a STORED decision
    // (a stored 0 must beat `PDO_AUTOCOMPLETE_TURN_END=1`, or unticking the box
    // would be a no-op on a machine that sets the env var).
    let state = test_state().await;

    let fresh = state.settings.view().await.unwrap();
    assert_eq!(fresh["autocomplete_turn_end"]["effective"], false);
    assert_eq!(fresh["autocomplete_turn_end"]["default"], false);
    assert!(fresh["autocomplete_turn_end"]["stored"].is_null());

    for (on, want_source) in [(true, "stored"), (false, "stored")] {
        instance_config::update(
            &state.db,
            UpdateInstanceConfig {
                autocomplete_turn_end: Some(on),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let view = state.settings.view().await.unwrap();
        assert_eq!(
            view["autocomplete_turn_end"]["effective"],
            serde_json::json!(on)
        );
        assert_eq!(
            view["autocomplete_turn_end"]["stored"],
            serde_json::json!(on),
            "the stored tier is disclosed as a bool, not as 0/1"
        );
        assert_eq!(
            view["autocomplete_turn_end"]["source"],
            serde_json::json!(want_source)
        );
    }
}

#[tokio::test]
async fn settings_view_discloses_the_default_auto_name_tiers() {
    // #338 / ADR-0015: `effective` comes from the SAME resolver the create
    // chokepoint consumes, the built-in default is ON, and both directions of an
    // explicit save are a STORED decision (a stored 0 must beat
    // `PDO_DEFAULT_AUTO_NAME=1`, or unticking the box would be a no-op with the env set).
    let state = test_state().await;

    let fresh = state.settings.view().await.unwrap();
    assert_eq!(fresh["default_auto_name"]["effective"], true);
    assert_eq!(fresh["default_auto_name"]["default"], true);
    assert!(fresh["default_auto_name"]["stored"].is_null());

    for on in [false, true] {
        instance_config::update(
            &state.db,
            UpdateInstanceConfig {
                default_auto_name: Some(on),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let view = state.settings.view().await.unwrap();
        assert_eq!(
            view["default_auto_name"]["effective"],
            serde_json::json!(on)
        );
        assert_eq!(
            view["default_auto_name"]["stored"],
            serde_json::json!(on),
            "the stored tier is disclosed as a bool, not as 0/1"
        );
        assert_eq!(
            view["default_auto_name"]["source"],
            serde_json::json!("stored")
        );
    }
}

#[tokio::test]
async fn get_settings_default_shape_stored_null() {
    // A fresh instance has an all-NULL row. Assert the robust invariants: the
    // per-field shape, the `default` tier's constants, and `stored == null`.
    // We deliberately do NOT assert `source`/`effective` for the cap here —
    // they read the process-global `PDO_SESSION_CAP`, which a concurrent
    // env-mutating unit test could transiently shadow (the resolver unit
    // tests own the env-precedence assertions).
    let state = test_state().await;
    let view = get_settings_json(&state).await;

    for field in ["session_cap", "reaper_ttl_secs", "guard_timeout_secs"] {
        let f = &view[field];
        assert!(
            f["stored"].is_null(),
            "{field}.stored must be null on a fresh row: {f}"
        );
        assert!(f.get("effective").is_some(), "{field}.effective missing");
        assert!(f.get("source").is_some(), "{field}.source missing");
        assert!(f.get("env").is_some(), "{field}.env key missing");
        assert!(f.get("default").is_some(), "{field}.default missing");
    }
    assert_eq!(view["session_cap"]["default"], 20);
    assert_eq!(view["reaper_ttl_secs"]["default"], 3600);
    assert_eq!(view["guard_timeout_secs"]["default"], 60);
    assert!(
        view["updated_at"].is_string(),
        "updated_at must be surfaced"
    );

    // default_model: string knob with no baked-in default. On a fresh
    // row (and no PDO_DEFAULT_MODEL) stored/effective/default are all null and
    // the source is "default" (= the account default, no --model).
    let dm = &view["default_model"];
    assert!(
        dm["stored"].is_null(),
        "default_model.stored must be null: {dm}"
    );
    assert!(
        dm["default"].is_null(),
        "default_model.default is always null: {dm}"
    );
    assert!(
        dm.get("effective").is_some(),
        "default_model.effective missing"
    );
    assert!(dm.get("source").is_some(), "default_model.source missing");
    assert!(dm.get("env").is_some(), "default_model.env key missing");
    // `env` reads the process-global PDO_DEFAULT_MODEL; a concurrent
    // env-mutating unit test could transiently shadow it, so we assert the
    // effective/source pairing only in the stored-wins tests below.

    // #471 AC1: the two sandbox-image knobs are GONE from the payload, along with the
    // `sandbox_image` tag disclosure that only existed to explain the Dockerfile field.
    // Asserted here, on the shape test, so a re-add cannot slip in as an extra key.
    for gone in ["image_source", "dockerfile_path", "sandbox_image"] {
        assert!(
            view.get(gone).is_none(),
            "#471 removed `{gone}` from GET /settings: {view}"
        );
    }
    // What is left on the sandbox side: the profile knob, the profile list, the probe.
    for kept in ["default_sandbox", "sandbox_profiles", "sandbox_docker"] {
        assert!(view.get(kept).is_some(), "`{kept}` must stay: {view}");
    }
}

#[tokio::test]
async fn settings_view_lists_harnesses_with_source_and_installed() {
    // #586 AC#1: `GET /settings → harness_descriptors` carries, per resolved
    // harness, `name` + `source` (builtin|descriptor) + `installed` (bool) — the
    // shape the two-section, grey-if-uninstalled picker reads. The embedded floor
    // (claude/opencode) always resolves as `builtin`; disk-declared harnesses (if
    // any on this host) ride as `descriptor`. `installed` reads the real `$PATH`,
    // so we assert its TYPE, never a value — the picker only greys on it. `rejected`
    // is preserved alongside (AC: kept for an eventual display).
    let state = test_state().await;
    let view = get_settings_json(&state).await;

    let harnesses = view["harness_descriptors"]["harnesses"]
        .as_array()
        .expect("harness_descriptors.harnesses must be an array");
    assert!(
        !harnesses.is_empty(),
        "the floor alone makes this non-empty"
    );

    for h in harnesses {
        assert!(h["name"].is_string(), "each harness carries a name: {h}");
        let source = h["source"].as_str().expect("source is a string");
        assert!(
            source == "builtin" || source == "descriptor",
            "source is builtin|descriptor, got {source:?}"
        );
        assert!(h["installed"].is_boolean(), "installed is a bool: {h}");
        // #616/ADR-0053: the offer deduced from the binary rides on each entry —
        // the model & effort catalogues (arrays, possibly empty = free-text
        // fallback), the served effort-axis fact (a bool, replacing the client's
        // hard-coded map), and the probed version (a string or null).
        assert!(h["models"].is_array(), "models is an array: {h}");
        assert!(h["efforts"].is_array(), "efforts is an array: {h}");
        assert!(h["has_effort"].is_boolean(), "has_effort is a bool: {h}");
        assert!(
            h["version"].is_string() || h["version"].is_null(),
            "version is a string or null: {h}"
        );
    }

    // The embedded floor always resolves, always as `builtin` — copilot and pi
    // are the third and fourth arms, listed under "Built-in" like the other two.
    for floor in ["claude", "opencode", "copilot", "pi"] {
        let entry = harnesses
            .iter()
            .find(|h| h["name"] == floor)
            .unwrap_or_else(|| panic!("floor harness `{floor}` must appear: {view}"));
        assert_eq!(entry["source"], "builtin", "`{floor}` is built-in");
    }

    assert!(
        view["harness_descriptors"]["rejected"].is_array(),
        "rejected stays an array: {view}"
    );
}

#[tokio::test]
async fn get_settings_surfaces_sandbox_docker_probe() {
    // The advisory probe is folded into GET /settings. `test_state` has no
    // docker_cmd_override, so the real `docker` (absent in CI) → available:false
    // with a reason. The shape must always be present regardless of the verdict.
    let state = test_state().await;
    let view = get_settings_json(&state).await;
    let sd = &view["sandbox_docker"];
    assert!(sd.get("available").is_some(), "available missing: {sd}");
    assert!(sd["available"].is_boolean(), "available must be bool: {sd}");
    assert!(
        sd.get("reason").is_some(),
        "reason key must be present: {sd}"
    );
    assert!(
        sd["checked_at"].is_string(),
        "checked_at must be a string: {sd}"
    );
}

#[tokio::test]
async fn refresh_models_answers_the_harness_entry_or_404() {
    let state = test_state().await;
    let (status, entry) = post_json(&state, "/settings/harnesses/opencode/refresh").await;
    assert_eq!(status, StatusCode::OK, "{entry}");
    assert_eq!(entry["name"], "opencode");
    assert!(entry["models"].is_array(), "{entry}");
    assert!(entry["installed"].is_boolean(), "{entry}");
    assert!(
        entry["probed_at"].is_string() || entry["probed_at"].is_null(),
        "{entry}"
    );

    let (status, body) = post_json(&state, "/settings/harnesses/ghost/refresh").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body["error"].as_str().unwrap().contains("ghost"), "{body}");
}
