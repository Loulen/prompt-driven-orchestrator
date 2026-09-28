//! Stage 3 (ADR-0080): the stored tier over a real SQLite — what an edit
//! writes, what `""` clears, what a checkbox stores, when `updated_at` moves.

use pdo_run::adapters::env::resolve_manager_enabled;
use pdo_settings::adapters::sqlite::instance_config::{get, update};
use pdo_settings::domain::instance_config::UpdateInstanceConfig;

use super::test_db;

#[tokio::test]
async fn fresh_row_is_all_null() {
    // Load-bearing (ADR-0015): a fresh install seeds an all-NULL row so the
    // resolver falls through to env → default. A non-null default here would
    // silently break the env-driven cap/TTL tests.
    let db = test_db().await;
    let cfg = get(&db).await.unwrap();
    assert_eq!(cfg.session_cap, None);
    assert_eq!(cfg.reaper_ttl_secs, None);
    assert_eq!(cfg.guard_timeout_secs, None);
    assert_eq!(cfg.default_model, None);
    assert_eq!(cfg.default_sandbox, None);
    assert_eq!(cfg.autocomplete_turn_end, None);
    assert_eq!(cfg.default_auto_name, None);
    assert_eq!(cfg.default_harness, None);
    assert!(cfg.default_harness_model.is_empty());
    assert_eq!(cfg.manager_enabled, None);
    assert_eq!(cfg.manager_profile, None);
    assert!(!cfg.updated_at.is_empty(), "seed must stamp updated_at");
}

#[tokio::test]
async fn update_sets_and_reads_back_default_harness() {
    let db = test_db().await;
    let updated = update(
        &db,
        UpdateInstanceConfig {
            default_harness: Some("opencode".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.default_harness.as_deref(), Some("opencode"));
    assert_eq!(
        get(&db).await.unwrap().default_harness.as_deref(),
        Some("opencode")
    );
    update(
        &db,
        UpdateInstanceConfig {
            default_harness: Some(String::new()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(get(&db).await.unwrap().default_harness, None);
}

#[tokio::test]
async fn update_sets_and_reads_back_per_harness_default_model() {
    let db = test_db().await;
    let map: std::collections::BTreeMap<String, String> = [
        ("claude".to_string(), "opus".to_string()),
        ("opencode".to_string(), "openrouter/foo".to_string()),
    ]
    .into_iter()
    .collect();
    update(
        &db,
        UpdateInstanceConfig {
            default_harness_model: Some(map.clone()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(get(&db).await.unwrap().default_harness_model, map);
}

/// Manager on demand: the auto-start flag is a set-only `0`/`1` knob — both
/// directions persist a stored decision. The resolver is pure over its
/// `stored` arg, so a stored `0` beats the env tier by construction (no env
/// manipulation here: parallel tests share the process environment).
#[tokio::test]
async fn update_sets_and_reads_back_manager_enabled_both_ways() {
    let db = test_db().await;
    let updated = update(
        &db,
        UpdateInstanceConfig {
            manager_enabled: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.manager_enabled, Some(1));
    assert!(resolve_manager_enabled(updated.manager_enabled));

    let off = update(
        &db,
        UpdateInstanceConfig {
            manager_enabled: Some(false),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(off.manager_enabled, Some(0));
    assert!(
        !resolve_manager_enabled(off.manager_enabled),
        "a stored 0 is a decision, not a fall-through"
    );
}

/// Manager on demand: the profile pin round-trips, `""` clears back to SQL
/// NULL (« Follow the Run »), and unset resolves to the default (false) with
/// no env tier speaking.
#[tokio::test]
async fn update_sets_and_clears_manager_profile() {
    let db = test_db().await;
    let updated = update(
        &db,
        UpdateInstanceConfig {
            manager_profile: Some("reviewer".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.manager_profile.as_deref(), Some("reviewer"));

    let cleared = update(
        &db,
        UpdateInstanceConfig {
            manager_profile: Some(String::new()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        cleared.manager_profile, None,
        "empty string must clear the pin back to NULL"
    );
    let raw: Option<String> =
        sqlx::query_scalar("SELECT manager_profile FROM instance_config WHERE id = 1")
            .fetch_optional(&db)
            .await
            .unwrap()
            .flatten();
    assert_eq!(raw, None, "clear must persist NULL, never ''");
}

/// #563/ADR-0057: the Instance tier of `AgentChoice` — a fresh row carries
/// none, round-trips a `Profile` reference, then clears back to `None` via
/// the `Some(None)` double-Option idiom (mirrors the Triggers' `harness`).
#[tokio::test]
async fn agent_choice_round_trips_and_clears() {
    let db = test_db().await;
    assert_eq!(get(&db).await.unwrap().agent_choice, None);

    let choice = pdo_harness::agent_choice::AgentChoice::Profile {
        profile_id: "reviewer".to_string(),
    };
    let updated = update(
        &db,
        UpdateInstanceConfig {
            agent_choice: Some(Some(choice.clone())),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.agent_choice, Some(choice.clone()));
    assert_eq!(get(&db).await.unwrap().agent_choice, Some(choice));

    // `Some(None)` clears it back to unset; `None` (the default) leaves it be.
    update(
        &db,
        UpdateInstanceConfig {
            agent_choice: Some(None),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(get(&db).await.unwrap().agent_choice, None);
}

/// #563: a `Custom` combination round-trips model/effort too, and an
/// `agent_choice`-only edit does not disturb `default_harness`.
#[tokio::test]
async fn agent_choice_custom_round_trips_alongside_legacy_default_harness() {
    let db = test_db().await;
    update(
        &db,
        UpdateInstanceConfig {
            default_harness: Some("claude".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let choice = pdo_harness::agent_choice::AgentChoice::Custom {
        harness: "opencode".to_string(),
        model: Some("gpt-5".to_string()),
        effort: Some("high".to_string()),
    };
    update(
        &db,
        UpdateInstanceConfig {
            agent_choice: Some(Some(choice.clone())),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let cfg = get(&db).await.unwrap();
    assert_eq!(cfg.agent_choice, Some(choice));
    assert_eq!(
        cfg.default_harness.as_deref(),
        Some("claude"),
        "an agent_choice-only edit must not touch the legacy field"
    );
}

#[tokio::test]
async fn update_sets_default_model() {
    let db = test_db().await;
    let updated = update(
        &db,
        UpdateInstanceConfig {
            default_model: Some("opus".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.default_model.as_deref(), Some("opus"));
    assert_eq!(
        get(&db).await.unwrap().default_model.as_deref(),
        Some("opus")
    );
    assert_eq!(updated.session_cap, None);
}

#[tokio::test]
async fn update_clears_default_model_on_empty_string() {
    // "" is the clear sentinel: it must reset the column to NULL, not persist
    // an empty string (which would win precedence and reach the tail as an
    // empty `--model`).
    let db = test_db().await;
    update(
        &db,
        UpdateInstanceConfig {
            default_model: Some("opus".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let cleared = update(
        &db,
        UpdateInstanceConfig {
            default_model: Some(String::new()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        cleared.default_model, None,
        "empty string must clear the stored model back to NULL"
    );
}

#[tokio::test]
async fn default_model_only_edit_is_not_a_noop() {
    // Guard-rail for the `is_empty()` addition: an edit that touches ONLY
    // default_model (all numeric knobs None) must still write. Without the
    // `default_model.is_none()` clause it would fall into the no-op branch
    // and silently never persist.
    let db = test_db().await;
    let updated = update(
        &db,
        UpdateInstanceConfig {
            default_model: Some("haiku".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.default_model.as_deref(), Some("haiku"));
}

#[tokio::test]
async fn update_sets_default_sandbox() {
    let db = test_db().await;
    let updated = update(
        &db,
        UpdateInstanceConfig {
            default_sandbox: Some("minimal".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.default_sandbox.as_deref(), Some("minimal"));
    assert_eq!(
        get(&db).await.unwrap().default_sandbox.as_deref(),
        Some("minimal")
    );
    assert_eq!(updated.default_model, None);
}

#[tokio::test]
async fn update_clears_default_sandbox_on_empty_string() {
    // "" is the clear sentinel: it resets the column to NULL (mirrors #347), so the
    // resolver falls back env→default(off).
    //
    // And the raw column really is SQL NULL, not `''`: a stored `''` would win precedence
    // and be treated as unset only by the empty-string filter downstream.
    let db = test_db().await;
    update(
        &db,
        UpdateInstanceConfig {
            default_sandbox: Some("minimal".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let cleared = update(
        &db,
        UpdateInstanceConfig {
            default_sandbox: Some(String::new()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        cleared.default_sandbox, None,
        "empty string must clear the stored default sandbox back to NULL"
    );
    let raw: Option<String> =
        sqlx::query_scalar("SELECT default_sandbox FROM instance_config WHERE id = 1")
            .fetch_optional(&db)
            .await
            .unwrap()
            .flatten();
    assert_eq!(raw, None, "clear must persist NULL, never ''");
}

#[tokio::test]
async fn default_sandbox_only_edit_is_not_a_noop() {
    // Guard-rail for the `is_empty()` addition: an edit touching ONLY
    // default_sandbox (all other knobs None) must still write.
    let db = test_db().await;
    let updated = update(
        &db,
        UpdateInstanceConfig {
            default_sandbox: Some("full".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.default_sandbox.as_deref(), Some("full"));
}

#[tokio::test]
async fn update_stores_autocomplete_turn_end_as_zero_or_one() {
    // #469: a bool on the wire, 0/1 in the column. Both directions PERSIST —
    // "off" must be a stored decision, not a fall-through, or unticking the
    // box could not override `PDO_AUTOCOMPLETE_TURN_END=1`.
    let db = test_db().await;

    let on = update(
        &db,
        UpdateInstanceConfig {
            autocomplete_turn_end: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(on.autocomplete_turn_end, Some(1));
    assert_eq!(get(&db).await.unwrap().autocomplete_turn_end, Some(1));

    let off = update(
        &db,
        UpdateInstanceConfig {
            autocomplete_turn_end: Some(false),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        off.autocomplete_turn_end,
        Some(0),
        "unticking must persist a stored 0, never NULL"
    );
    assert_eq!(off.session_cap, None);
}

#[tokio::test]
async fn autocomplete_turn_end_only_edit_is_not_a_noop() {
    // Guard-rail for the `is_empty()` addition: an edit touching ONLY this
    // knob must still write. Without the clause it would fall into the no-op
    // branch and the checkbox would silently never persist.
    let db = test_db().await;
    let updated = update(
        &db,
        UpdateInstanceConfig {
            autocomplete_turn_end: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.autocomplete_turn_end, Some(1));
}

#[tokio::test]
async fn update_stores_default_auto_name_as_zero_or_one() {
    // #338: a bool on the wire, 0/1 in the column. Both directions PERSIST —
    // "off" must be a stored decision, not a fall-through, or unticking the box
    // could not override `PDO_DEFAULT_AUTO_NAME=1`.
    let db = test_db().await;

    let on = update(
        &db,
        UpdateInstanceConfig {
            default_auto_name: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(on.default_auto_name, Some(1));
    assert_eq!(get(&db).await.unwrap().default_auto_name, Some(1));

    let off = update(
        &db,
        UpdateInstanceConfig {
            default_auto_name: Some(false),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        off.default_auto_name,
        Some(0),
        "unticking must persist a stored 0, never NULL"
    );
    assert_eq!(off.session_cap, None);
}

#[tokio::test]
async fn default_auto_name_only_edit_is_not_a_noop() {
    // Guard-rail for the `is_empty()` addition: an edit touching ONLY this knob
    // must still write. Without the clause it would fall into the no-op branch
    // and the checkbox would silently never persist.
    let db = test_db().await;
    let updated = update(
        &db,
        UpdateInstanceConfig {
            default_auto_name: Some(false),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.default_auto_name, Some(0));
}

#[tokio::test]
async fn update_sets_only_the_given_fields() {
    let db = test_db().await;
    let updated = update(
        &db,
        UpdateInstanceConfig {
            session_cap: Some(4),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.session_cap, Some(4));
    assert_eq!(updated.reaper_ttl_secs, None);
    assert_eq!(updated.guard_timeout_secs, None);

    let updated = update(
        &db,
        UpdateInstanceConfig {
            reaper_ttl_secs: Some(120),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.session_cap, Some(4));
    assert_eq!(updated.reaper_ttl_secs, Some(120));
}

#[tokio::test]
async fn update_bumps_updated_at() {
    let db = test_db().await;
    let before = get(&db).await.unwrap().updated_at;
    // now_iso() is millisecond-resolution; ensure a distinct instant.
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    let after = update(
        &db,
        UpdateInstanceConfig {
            guard_timeout_secs: Some(30),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_ne!(after.updated_at, before, "a real edit must bump updated_at");
}

#[tokio::test]
async fn empty_update_is_a_noop_and_leaves_updated_at() {
    let db = test_db().await;
    let before = get(&db).await.unwrap();
    let after = update(&db, UpdateInstanceConfig::default()).await.unwrap();
    assert_eq!(after, before, "a no-op edit must not mutate the row");
}

#[tokio::test]
async fn get_is_idempotent() {
    let db = test_db().await;
    assert_eq!(get(&db).await.unwrap(), get(&db).await.unwrap());
}
