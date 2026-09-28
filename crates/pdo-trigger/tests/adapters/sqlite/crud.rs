use pdo_trigger::adapters::sqlite::*;
use pdo_trigger::domain::trigger::{FireRecord, UpdateTrigger};

use super::{sample, test_db};

#[tokio::test]
async fn create_then_get_round_trips() {
    let db = test_db().await;
    let created = create(&db, sample("nightly audit", "0 9 * * *"))
        .await
        .unwrap();
    let fetched = get(&db, &created.id).await.unwrap().unwrap();
    assert_eq!(fetched, created);
    assert_eq!(fetched.name, "nightly audit");
    assert_eq!(fetched.cron, "0 9 * * *");
    assert!(fetched.enabled);
    assert_eq!(fetched.variables, serde_json::json!({"depth": "full"}));
}

#[tokio::test]
async fn get_missing_returns_none() {
    let db = test_db().await;
    assert!(get(&db, "trg-nope").await.unwrap().is_none());
}

#[tokio::test]
async fn list_returns_all_triggers() {
    let db = test_db().await;
    create(&db, sample("a", "0 9 * * *")).await.unwrap();
    create(&db, sample("b", "*/15 * * * *")).await.unwrap();
    let all = list(&db).await.unwrap();
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn delete_removes_a_trigger() {
    let db = test_db().await;
    let t = create(&db, sample("doomed", "0 9 * * *")).await.unwrap();
    assert!(delete(&db, &t.id).await.unwrap());
    assert!(get(&db, &t.id).await.unwrap().is_none());
    // Deleting again is a no-op.
    assert!(!delete(&db, &t.id).await.unwrap());
}

#[tokio::test]
async fn due_triggers_selects_enabled_and_past_due() {
    let db = test_db().await;
    // Due (next_fire in the past) and enabled.
    let mut due = sample("due", "* * * * *");
    due.next_fire_at = Some("2020-01-01T00:00:00.000Z".to_string());
    let due = create(&db, due).await.unwrap();
    // Not yet due (next_fire in the future).
    let mut future = sample("future", "* * * * *");
    future.next_fire_at = Some("2999-01-01T00:00:00.000Z".to_string());
    create(&db, future).await.unwrap();
    // Due but disabled.
    let mut disabled = sample("disabled", "* * * * *");
    disabled.next_fire_at = Some("2020-01-01T00:00:00.000Z".to_string());
    let disabled = create(&db, disabled).await.unwrap();
    update(
        &db,
        &disabled.id,
        UpdateTrigger {
            enabled: Some(false),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let selected = due_triggers(&db, "2026-06-06T10:00:00.000Z").await.unwrap();
    let ids: Vec<&str> = selected.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, vec![due.id.as_str()]);
}

#[tokio::test]
async fn due_triggers_includes_a_past_due_local_offset_row() {
    // Regression for #222. A row stored with a local offset (a CEST box's
    // `chrono::Local::now()` before the UTC write-side fix) represents an
    // instant already in the past, yet sorts lexicographically *after* the
    // UTC now-string. A raw string compare drops it → silent dormancy; the
    // tz-normalised `julianday` compare keeps it. Fails pre-fix on any host
    // (it does not rely on the test machine being non-UTC).
    let db = test_db().await;
    let mut t = sample("legacy-offset", "* * * * *");
    // 19:15 +02:00 == 17:15Z, i.e. before the 17:30Z "now" → genuinely due.
    t.next_fire_at = Some("2026-06-18T19:15:00.000+02:00".to_string());
    let t = create(&db, t).await.unwrap();

    let now = "2026-06-18T17:30:00.000Z";
    // The bug's precondition: the stored string sorts *after* `now`.
    assert!(
        t.next_fire_at.as_deref().unwrap() > now,
        "precondition: the local-offset string must sort after the UTC now-string"
    );

    let due = due_triggers(&db, now).await.unwrap();
    assert_eq!(
        due.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
        vec![t.id.as_str()],
        "a past-due local-offset row must still be selected (tz-normalised compare)"
    );
}

#[tokio::test]
async fn due_triggers_orders_by_instant_not_string() {
    // Ordering must also be tz-normalised: an earlier instant carried on a
    // `+02:00` offset must sort before a later `…Z` instant even though it
    // sorts *after* it as a string.
    let db = test_db().await;
    let mut early = sample("early", "* * * * *");
    early.next_fire_at = Some("2026-06-18T19:00:00.000+02:00".to_string()); // 17:00Z
    let early = create(&db, early).await.unwrap();
    let mut late = sample("late", "* * * * *");
    late.next_fire_at = Some("2026-06-18T17:30:00.000Z".to_string()); // 17:30Z
    let late = create(&db, late).await.unwrap();

    let due = due_triggers(&db, "2026-06-18T18:00:00.000Z").await.unwrap();
    assert_eq!(
        due.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
        vec![early.id.as_str(), late.id.as_str()],
        "due triggers must be ordered by instant (17:00Z before 17:30Z), not by string"
    );
}

#[tokio::test]
async fn record_fire_writes_audit_and_rolls_up_outcome() {
    let db = test_db().await;
    let t = create(&db, sample("audited", "* * * * *")).await.unwrap();

    record_fire(
        &db,
        &t.id,
        &FireRecord {
            outcome: "skipped-overlap".to_string(),
            reason: Some("previous run still active".to_string()),
            run_id: None,
            guard_stdout: None,
            guard_stderr: None,
            guard_exit_code: None,
            source: None,
        },
    )
    .await
    .unwrap();
    record_fire(
        &db,
        &t.id,
        &FireRecord {
            outcome: "fired".to_string(),
            reason: None,
            run_id: Some("20260606-100000-abc1234".to_string()),
            guard_stdout: None,
            guard_stderr: None,
            guard_exit_code: None,
            source: None,
        },
    )
    .await
    .unwrap();

    let history = fire_history(&db, &t.id).await.unwrap();
    assert_eq!(history.len(), 2);
    // Newest first.
    assert_eq!(history[0].outcome, "fired");
    assert_eq!(
        history[0].run_id.as_deref(),
        Some("20260606-100000-abc1234")
    );
    assert_eq!(history[1].outcome, "skipped-overlap");

    let after = get(&db, &t.id).await.unwrap().unwrap();
    assert_eq!(after.last_outcome.as_deref(), Some("fired"));
    assert!(after.last_fired_at.is_some());
}

#[tokio::test]
async fn non_fire_outcome_updates_last_outcome_but_not_last_fired_at() {
    let db = test_db().await;
    let t = create(&db, sample("skippy", "* * * * *")).await.unwrap();
    record_fire(
        &db,
        &t.id,
        &FireRecord {
            outcome: "guard-exit-nonzero".to_string(),
            reason: Some("no work".to_string()),
            run_id: None,
            guard_stdout: None,
            guard_stderr: None,
            guard_exit_code: None,
            source: None,
        },
    )
    .await
    .unwrap();
    let after = get(&db, &t.id).await.unwrap().unwrap();
    assert_eq!(after.last_outcome.as_deref(), Some("guard-exit-nonzero"));
    assert!(after.last_fired_at.is_none());
}

/// #368: fire history and the `last_fired_at`/`last_outcome` rollup are keyed
/// by *Trigger*, not by the pipeline the Trigger fires. Every other test
/// creates a single Trigger, so this isolation is unproven — under a
/// per-pipeline bug, two Triggers on the SAME pipeline could share history or
/// clobber each other's rollup. This drives both directions: A's fires must
/// never appear in B's history or move B's rollup, and vice-versa.
#[tokio::test]
async fn fire_history_and_rollups_are_isolated_per_trigger() {
    let db = test_db().await;
    // Two distinct Triggers on the SAME library pipeline ("lib-pipe-1").
    let a = create(&db, sample("alpha", "* * * * *")).await.unwrap();
    let b = create(&db, sample("bravo", "*/5 * * * *")).await.unwrap();
    assert_ne!(a.id, b.id, "the two triggers must be distinct rows");
    assert_eq!(a.pipeline_id, "lib-pipe-1");
    assert_eq!(b.pipeline_id, "lib-pipe-1");

    // --- Fire ONLY trigger A: one real fire, then a non-fire. ---
    record_fire(
        &db,
        &a.id,
        &FireRecord {
            outcome: "fired".to_string(),
            reason: None,
            run_id: Some("run-a-fired".to_string()),
            guard_stdout: None,
            guard_stderr: None,
            guard_exit_code: None,
            source: None,
        },
    )
    .await
    .unwrap();
    record_fire(
        &db,
        &a.id,
        &FireRecord {
            outcome: "skipped-overlap".to_string(),
            reason: Some("previous run still active".to_string()),
            run_id: None,
            guard_stdout: None,
            guard_stderr: None,
            guard_exit_code: None,
            source: None,
        },
    )
    .await
    .unwrap();

    // A has both rows; B's history is still empty (no bleed across Triggers).
    let hist_a = fire_history(&db, &a.id).await.unwrap();
    assert_eq!(hist_a.len(), 2);
    assert!(
        hist_a.iter().all(|f| f.trigger_id == a.id),
        "every A-history row must be keyed to A"
    );
    assert!(
        fire_history(&db, &b.id).await.unwrap().is_empty(),
        "firing A must not add rows to B's history"
    );

    // A's rollup reflects A's fire; B's rollup is untouched (None/None).
    let a1 = get(&db, &a.id).await.unwrap().unwrap();
    assert_eq!(a1.last_outcome.as_deref(), Some("skipped-overlap"));
    assert!(a1.last_fired_at.is_some(), "A fired at least once");
    let b1 = get(&db, &b.id).await.unwrap().unwrap();
    assert!(
        b1.last_fired_at.is_none() && b1.last_outcome.is_none(),
        "firing A must not roll up onto B"
    );

    // --- Now fire trigger B once, with a distinct run id. ---
    record_fire(
        &db,
        &b.id,
        &FireRecord {
            outcome: "fired".to_string(),
            reason: None,
            run_id: Some("run-b-fired".to_string()),
            guard_stdout: None,
            guard_stderr: None,
            guard_exit_code: None,
            source: None,
        },
    )
    .await
    .unwrap();

    // Histories stay disjoint and correctly counted.
    let hist_a = fire_history(&db, &a.id).await.unwrap();
    let hist_b = fire_history(&db, &b.id).await.unwrap();
    assert_eq!(hist_a.len(), 2, "B's fire must not leak into A's history");
    assert_eq!(hist_b.len(), 1);
    assert!(hist_b.iter().all(|f| f.trigger_id == b.id));
    let a_run_ids: Vec<_> = hist_a.iter().filter_map(|f| f.run_id.as_deref()).collect();
    let b_run_ids: Vec<_> = hist_b.iter().filter_map(|f| f.run_id.as_deref()).collect();
    assert_eq!(a_run_ids, vec!["run-a-fired"]);
    assert_eq!(b_run_ids, vec!["run-b-fired"]);

    // B's rollup now reflects B; A's rollup is unchanged by B's fire.
    let a2 = get(&db, &a.id).await.unwrap().unwrap();
    let b2 = get(&db, &b.id).await.unwrap().unwrap();
    assert_eq!(
        a2.last_outcome.as_deref(),
        Some("skipped-overlap"),
        "firing B must not touch A's rollup"
    );
    assert_eq!(b2.last_outcome.as_deref(), Some("fired"));
    assert!(b2.last_fired_at.is_some());
}

#[tokio::test]
async fn set_next_fire_updates_schedule() {
    let db = test_db().await;
    let t = create(&db, sample("rescheduled", "* * * * *"))
        .await
        .unwrap();
    set_next_fire(&db, &t.id, Some("2027-01-01T00:00:00.000Z"))
        .await
        .unwrap();
    let after = get(&db, &t.id).await.unwrap().unwrap();
    assert_eq!(
        after.next_fire_at.as_deref(),
        Some("2027-01-01T00:00:00.000Z")
    );
    // A broken reference can clear next_fire so the trigger stops firing.
    set_next_fire(&db, &t.id, None).await.unwrap();
    assert!(get(&db, &t.id)
        .await
        .unwrap()
        .unwrap()
        .next_fire_at
        .is_none());
}

#[tokio::test]
async fn update_edits_config_fields_and_leaves_others_untouched() {
    let db = test_db().await;
    let t = create(&db, sample("editable", "0 9 * * *")).await.unwrap();

    // Edit the schedule, input template and overlap policy; recompute the
    // next fire. Fields left `None` keep their prior value.
    update(
        &db,
        &t.id,
        UpdateTrigger {
            cron: Some("*/15 * * * *".to_string()),
            input_template: Some("new instruction".to_string()),
            overlap_policy: Some("allow".to_string()),
            next_fire_at: Some(Some("2027-03-01T00:00:00.000Z".to_string())),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let after = get(&db, &t.id).await.unwrap().unwrap();
    assert_eq!(after.cron, "*/15 * * * *");
    assert_eq!(after.input_template, "new instruction");
    assert_eq!(after.overlap_policy, "allow");
    assert_eq!(
        after.next_fire_at.as_deref(),
        Some("2027-03-01T00:00:00.000Z")
    );
    // Untouched fields survive.
    assert_eq!(after.name, "editable");
    assert_eq!(after.pipeline_id, "lib-pipe-1");
    assert_eq!(after.target_repo.as_deref(), Some("/repos/foo"));
    assert!(after.enabled);
}

#[tokio::test]
async fn update_can_clear_a_nullable_field() {
    let db = test_db().await;
    let mut s = sample("guarded", "0 9 * * *");
    s.guard_command = Some("gh issue list".to_string());
    let t = create(&db, s).await.unwrap();
    assert_eq!(
        get(&db, &t.id)
            .await
            .unwrap()
            .unwrap()
            .guard_command
            .as_deref(),
        Some("gh issue list")
    );

    // Some(None) clears the guard to NULL; an unrelated edit leaves it alone.
    update(
        &db,
        &t.id,
        UpdateTrigger {
            guard_command: Some(None),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(get(&db, &t.id)
        .await
        .unwrap()
        .unwrap()
        .guard_command
        .is_none());
}

#[tokio::test]
async fn update_can_repoint_pipeline() {
    // #230: a Trigger must be movable to a different pipeline. Both the id and
    // the denormalised display name update together; unrelated fields survive.
    let db = test_db().await;
    let t = create(&db, sample("repointable", "0 9 * * *"))
        .await
        .unwrap();
    assert_eq!(t.pipeline_id, "lib-pipe-1");
    assert_eq!(t.pipeline_name, "Auditor");

    update(
        &db,
        &t.id,
        UpdateTrigger {
            pipeline_id: Some("lib-pipe-2".to_string()),
            pipeline_name: Some("Bugfixer".to_string()),
            next_fire_at: Some(Some("2027-03-01T00:00:00.000Z".to_string())),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let after = get(&db, &t.id).await.unwrap().unwrap();
    assert_eq!(after.pipeline_id, "lib-pipe-2");
    assert_eq!(after.pipeline_name, "Bugfixer");
    // A revived next fire (the dormant-on-rename recovery path).
    assert_eq!(
        after.next_fire_at.as_deref(),
        Some("2027-03-01T00:00:00.000Z")
    );
    // Everything else is left untouched.
    assert_eq!(after.name, "repointable");
    assert_eq!(after.cron, "0 9 * * *");
    assert_eq!(after.input_template, "audit the codebase");
    assert_eq!(after.target_repo.as_deref(), Some("/repos/foo"));
    assert!(after.enabled);
}

#[tokio::test]
async fn update_with_no_fields_is_a_noop() {
    let db = test_db().await;
    let t = create(&db, sample("stable", "0 9 * * *")).await.unwrap();
    update(&db, &t.id, UpdateTrigger::default()).await.unwrap();
    assert_eq!(get(&db, &t.id).await.unwrap().unwrap(), t);
}

/// #372: the enable/disable bit now toggles through `update()` (folded into
/// the single atomic write). Disabling excludes a due Trigger from
/// `due_triggers`; re-enabling brings it back.
#[tokio::test]
async fn update_enabled_toggles_and_pauses_firing() {
    let db = test_db().await;
    let mut due = sample("toggle", "* * * * *");
    due.next_fire_at = Some("2020-01-01T00:00:00.000Z".to_string());
    let t = create(&db, due).await.unwrap();
    update(
        &db,
        &t.id,
        UpdateTrigger {
            enabled: Some(false),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(!get(&db, &t.id).await.unwrap().unwrap().enabled);
    // A disabled-but-due trigger is excluded from due_triggers.
    assert!(due_triggers(&db, "2026-06-06T10:00:00.000Z")
        .await
        .unwrap()
        .is_empty());
    update(
        &db,
        &t.id,
        UpdateTrigger {
            enabled: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(get(&db, &t.id).await.unwrap().unwrap().enabled);
}

/// #372: `update()` writes the enable bit and a forward `next_fire_at` in one
/// atomic UPDATE (the fold that replaced the separate `set_enabled` write).
#[tokio::test]
async fn update_writes_enabled_and_next_fire_atomically() {
    let db = test_db().await;
    let mut t = sample("atomic", "* * * * *");
    t.next_fire_at = Some("2020-01-01T00:00:00.000Z".to_string()); // stale/past
    let t = create(&db, t).await.unwrap();

    // `create()` always makes an enabled Trigger; disable it first.
    update(
        &db,
        &t.id,
        UpdateTrigger {
            enabled: Some(false),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Re-enable AND move next_fire forward in a single UPDATE (the #372 fold).
    update(
        &db,
        &t.id,
        UpdateTrigger {
            enabled: Some(true),
            next_fire_at: Some(Some("2027-01-01T09:00:00.000Z".to_string())),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let fetched = get(&db, &t.id).await.unwrap().unwrap();
    assert!(fetched.enabled, "enable bit must be written");
    assert_eq!(
        fetched.next_fire_at.as_deref(),
        Some("2027-01-01T09:00:00.000Z"),
        "next_fire_at must be written in the same update"
    );
}

// --- #239: bounded-`allow` max_concurrent persistence ---
