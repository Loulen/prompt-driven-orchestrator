use pdo_trigger::adapters::sqlite::*;
use pdo_trigger::domain::trigger::UpdateTrigger;

use super::{sample, test_db};

#[tokio::test]
async fn create_then_get_round_trips_max_concurrent() {
    let db = test_db().await;

    let mut bounded = sample("bounded", "0 9 * * *");
    bounded.overlap_policy = "allow".to_string();
    bounded.max_concurrent = Some(3);
    let created = create(&db, bounded).await.unwrap();
    assert_eq!(created.max_concurrent, Some(3));
    assert_eq!(
        get(&db, &created.id).await.unwrap().unwrap().max_concurrent,
        Some(3)
    );

    // The default (unbounded) round-trips as NULL.
    let unbounded = create(&db, sample("unbounded", "0 9 * * *")).await.unwrap();
    assert_eq!(unbounded.max_concurrent, None);
    assert_eq!(
        get(&db, &unbounded.id)
            .await
            .unwrap()
            .unwrap()
            .max_concurrent,
        None
    );
}

#[tokio::test]
async fn update_sets_and_clears_max_concurrent() {
    let db = test_db().await;
    let t = create(&db, sample("capped", "0 9 * * *")).await.unwrap();
    assert_eq!(t.max_concurrent, None);

    // Some(Some(n)) sets it.
    update(
        &db,
        &t.id,
        UpdateTrigger {
            max_concurrent: Some(Some(5)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        get(&db, &t.id).await.unwrap().unwrap().max_concurrent,
        Some(5)
    );

    // An unrelated edit (None) leaves it untouched.
    update(
        &db,
        &t.id,
        UpdateTrigger {
            input_template: Some("changed".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        get(&db, &t.id).await.unwrap().unwrap().max_concurrent,
        Some(5)
    );

    // Some(None) clears it back to NULL.
    update(
        &db,
        &t.id,
        UpdateTrigger {
            max_concurrent: Some(None),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(get(&db, &t.id).await.unwrap().unwrap().max_concurrent, None);
}

// --- #410: per-Trigger sandbox persistence (mirror of max_concurrent) ---

#[tokio::test]
async fn create_then_get_round_trips_sandbox() {
    let db = test_db().await;

    let mut sandboxed = sample("sandboxed", "0 9 * * *");
    sandboxed.sandbox = Some("minimal".to_string());
    let created = create(&db, sandboxed).await.unwrap();
    assert_eq!(created.sandbox.as_deref(), Some("minimal"));
    assert_eq!(
        get(&db, &created.id)
            .await
            .unwrap()
            .unwrap()
            .sandbox
            .as_deref(),
        Some("minimal")
    );

    // The default (inherit instance) round-trips as NULL.
    let inherit = create(&db, sample("inherit", "0 9 * * *")).await.unwrap();
    assert_eq!(inherit.sandbox, None);
    assert_eq!(get(&db, &inherit.id).await.unwrap().unwrap().sandbox, None);
}

#[tokio::test]
async fn update_sets_and_clears_sandbox() {
    let db = test_db().await;
    let t = create(&db, sample("sbx", "0 9 * * *")).await.unwrap();
    assert_eq!(t.sandbox, None);

    // Some(Some(mode)) sets it.
    update(
        &db,
        &t.id,
        UpdateTrigger {
            sandbox: Some(Some("full".to_string())),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        get(&db, &t.id).await.unwrap().unwrap().sandbox.as_deref(),
        Some("full")
    );

    // An unrelated edit (None) leaves it untouched.
    update(
        &db,
        &t.id,
        UpdateTrigger {
            input_template: Some("changed".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        get(&db, &t.id).await.unwrap().unwrap().sandbox.as_deref(),
        Some("full")
    );

    // Some(None) clears it back to NULL (inherit the instance default).
    update(
        &db,
        &t.id,
        UpdateTrigger {
            sandbox: Some(None),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(get(&db, &t.id).await.unwrap().unwrap().sandbox, None);
}

#[tokio::test]
async fn create_then_get_round_trips_auto_name() {
    // #338: the flag persists as 0/1 and reads back as a bool. Default (from the
    // sample helper) is `true`, and an explicit `false` round-trips.
    let db = test_db().await;

    let on = create(&db, sample("named", "0 9 * * *")).await.unwrap();
    assert!(on.auto_name, "sample() defaults to auto-name ON");
    assert!(get(&db, &on.id).await.unwrap().unwrap().auto_name);

    let mut manual = sample("manual", "0 9 * * *");
    manual.auto_name = false;
    let off = create(&db, manual).await.unwrap();
    assert!(!off.auto_name, "an explicit false must persist");
    assert!(!get(&db, &off.id).await.unwrap().unwrap().auto_name);
}

#[tokio::test]
async fn update_toggles_auto_name() {
    // #338: a flat toggle (mirror of `enabled`) — `Some(v)` sets, `None` leaves.
    let db = test_db().await;
    let t = create(&db, sample("t", "0 9 * * *")).await.unwrap();
    assert!(t.auto_name);

    // Some(false) turns it off.
    update(
        &db,
        &t.id,
        UpdateTrigger {
            auto_name: Some(false),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(!get(&db, &t.id).await.unwrap().unwrap().auto_name);

    // An unrelated edit (None) leaves it off.
    update(
        &db,
        &t.id,
        UpdateTrigger {
            input_template: Some("changed".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(!get(&db, &t.id).await.unwrap().unwrap().auto_name);

    // Some(true) turns it back on.
    update(
        &db,
        &t.id,
        UpdateTrigger {
            auto_name: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(get(&db, &t.id).await.unwrap().unwrap().auto_name);
}

/// #551 (ADR-0046): the per-Trigger harness round-trips through create, is set and
/// cleared through the double-`Option` update, and an unrelated edit leaves it. Mirror
/// of the `sandbox` double-`Option` semantics.
#[tokio::test]
async fn harness_round_trips_and_clears() {
    let db = test_db().await;
    let mut new = sample("t", "0 9 * * *");
    new.harness = Some("opencode".to_string());
    let t = create(&db, new).await.unwrap();
    assert_eq!(t.harness.as_deref(), Some("opencode"));

    // Some(Some(name)) sets it to another harness.
    update(
        &db,
        &t.id,
        UpdateTrigger {
            harness: Some(Some("claude".to_string())),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        get(&db, &t.id).await.unwrap().unwrap().harness.as_deref(),
        Some("claude")
    );

    // An unrelated edit (harness = None) leaves it untouched.
    update(
        &db,
        &t.id,
        UpdateTrigger {
            input_template: Some("changed".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        get(&db, &t.id).await.unwrap().unwrap().harness.as_deref(),
        Some("claude")
    );

    // Some(None) clears it back to inheriting the instance default.
    update(
        &db,
        &t.id,
        UpdateTrigger {
            harness: Some(None),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(get(&db, &t.id).await.unwrap().unwrap().harness, None);
}

/// #563 (ADR-0057): the per-Trigger `AgentChoice` round-trips through create, is
/// set and cleared through the double-`Option` update, and an unrelated edit
/// leaves it. Mirror of `harness_round_trips_and_clears` above.
#[tokio::test]
async fn agent_choice_round_trips_and_clears() {
    use pdo_harness::agent_choice::AgentChoice;

    let db = test_db().await;
    let mut new = sample("t", "0 9 * * *");
    new.agent_choice = Some(AgentChoice::Profile {
        profile_id: "prof-1".to_string(),
    });
    let t = create(&db, new).await.unwrap();
    assert_eq!(
        t.agent_choice,
        Some(AgentChoice::Profile {
            profile_id: "prof-1".to_string()
        })
    );

    // Some(Some(choice)) sets it to another choice.
    update(
        &db,
        &t.id,
        UpdateTrigger {
            agent_choice: Some(Some(AgentChoice::Custom {
                harness: "opencode".to_string(),
                model: None,
                effort: None,
            })),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        get(&db, &t.id).await.unwrap().unwrap().agent_choice,
        Some(AgentChoice::Custom {
            harness: "opencode".to_string(),
            model: None,
            effort: None,
        })
    );

    // An unrelated edit (agent_choice = None) leaves it untouched.
    update(
        &db,
        &t.id,
        UpdateTrigger {
            input_template: Some("changed".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        get(&db, &t.id).await.unwrap().unwrap().agent_choice,
        Some(AgentChoice::Custom {
            harness: "opencode".to_string(),
            model: None,
            effort: None,
        })
    );

    // Some(None) clears it back to the legacy `harness` field deciding again.
    update(
        &db,
        &t.id,
        UpdateTrigger {
            agent_choice: Some(None),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(get(&db, &t.id).await.unwrap().unwrap().agent_choice, None);
}

/// #563: a Trigger can carry both a legacy `harness` string AND an
/// `agent_choice`, independently — the two fields coexist on the row exactly
/// like `instance_config`'s and `project_store`'s equivalent tests, since
/// `pdo_harness::agent_choice::resolve` (not this store) decides which one wins.
#[tokio::test]
async fn agent_choice_and_legacy_harness_coexist() {
    use pdo_harness::agent_choice::AgentChoice;

    let db = test_db().await;
    let mut new = sample("t", "0 9 * * *");
    new.harness = Some("claude".to_string());
    new.agent_choice = Some(AgentChoice::Custom {
        harness: "opencode".to_string(),
        model: Some("gpt-5".to_string()),
        effort: Some("high".to_string()),
    });
    let t = create(&db, new).await.unwrap();

    let fetched = get(&db, &t.id).await.unwrap().unwrap();
    assert_eq!(fetched.harness.as_deref(), Some("claude"));
    assert_eq!(
        fetched.agent_choice,
        Some(AgentChoice::Custom {
            harness: "opencode".to_string(),
            model: Some("gpt-5".to_string()),
            effort: Some("high".to_string()),
        })
    );
}
