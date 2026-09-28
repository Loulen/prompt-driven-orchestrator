//! Combine, Uncombine and the rename-born absorption.

use pdo_kernel::{Actor, ResourceKind, Verb};
use pdo_stats::application::absorptions::combine::CombineError;
use pdo_stats::application::absorptions::uncombine::{UncombineCommand, UncombineError};
use pdo_stats::domain::absorption::request::{CombineRequest, CombineRow};
use pdo_stats::domain::absorption::{couple_scope, Named, COUPLE, NODE, PIPELINE};

use crate::fakes::{row, Build, Host, Store};

fn combine_row(key: &str) -> CombineRow {
    CombineRow {
        key: key.into(),
        name: key.to_uppercase(),
        scope: None,
    }
}

fn request(dimension: &str, scope: &str, absorbent: &str, members: &[&str]) -> CombineRequest {
    CombineRequest {
        dimension: dimension.into(),
        scope: scope.into(),
        scope_name: String::new(),
        absorbent: combine_row(absorbent),
        members: members.iter().map(|m| combine_row(m)).collect(),
    }
}

fn uncombine(dimension: &str, scope: &str, member: &str) -> UncombineCommand {
    UncombineCommand {
        dimension: dimension.into(),
        scope: scope.into(),
        member: member.into(),
    }
}

#[tokio::test]
async fn a_refused_combine_writes_nothing_and_audits_nothing() {
    let world = Build {
        host: Host::refusing(),
        ..Build::default()
    }
    .world();
    let refused = world
        .stats
        .combine()
        .execute(
            &Actor::LocalOperator,
            "ui",
            request(PIPELINE, "", "b", &["a"]),
        )
        .await;
    assert!(
        matches!(refused, Err(CombineError::Refused(_))),
        "{refused:?}"
    );
    assert!(world.store.writes().is_empty());
    assert!(world.audit.0.lock().unwrap().is_empty());
    let asked = world.host.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].resource, ResourceKind::StatsAbsorption);
    assert_eq!(asked[0].verb, Verb::Create);
}

#[tokio::test]
async fn a_combine_writes_audits_and_answers_the_list() {
    let world = Build::default().world();
    let list = world
        .stats
        .combine()
        .execute(
            &Actor::LocalOperator,
            "ui",
            request(PIPELINE, "", "b", &["a"]),
        )
        .await
        .unwrap();
    assert_eq!(
        world.store.writes(),
        ["combine pipeline [] b <- a (manual)"]
    );
    assert_eq!(list.absorptions.len(), 1);
    let audit = world.audit.0.lock().unwrap();
    assert_eq!(audit[0].action, "stats_absorption.combine");
    assert_eq!(audit[0].actor_hint, "ui");
    assert_eq!(audit[0].target_id.as_deref(), Some("b"));
}

#[tokio::test]
async fn a_combine_that_does_not_fit_is_refused_before_any_write() {
    let world = Build {
        store: Store::holding(vec![row(PIPELINE, "", "p", "keeper")]),
        ..Build::default()
    }
    .world();
    let combine = |body| {
        world
            .stats
            .combine()
            .execute(&Actor::LocalOperator, "ui", body)
    };
    let unknown = combine(request("project", "", "b", &["a"])).await;
    assert!(
        matches!(unknown, Err(CombineError::Invalid(_))),
        "{unknown:?}"
    );
    let absorbed_scope = combine(request(NODE, "p", "b", &["a"])).await;
    match absorbed_scope {
        Err(CombineError::Invalid(message)) => assert_eq!(
            message,
            "this pipeline is combined into another: combine its nodes there"
        ),
        other => panic!("{other:?}"),
    }
    let alone = combine(request(PIPELINE, "", "b", &["b"])).await;
    assert!(matches!(alone, Err(CombineError::Invalid(_))), "{alone:?}");
    assert!(world.store.writes().is_empty());
}

#[tokio::test]
async fn a_couple_absorption_is_written_over_the_raw_scopes_it_covers_now() {
    let mut build = Build {
        store: Store::holding(vec![row(PIPELINE, "", "q", "p")]),
        ..Build::default()
    };
    build
        .events
        .ran
        .insert("p".into(), ["coder".to_string()].into());
    build
        .events
        .ran
        .insert("q".into(), ["other".to_string()].into());
    let world = build.world();
    world
        .stats
        .combine()
        .execute(
            &Actor::LocalOperator,
            "ui",
            request(COUPLE, &couple_scope("p", "coder"), "m|high", &["m|"]),
        )
        .await
        .unwrap();
    // `q` never ran `coder`: its scope is not covered.
    assert_eq!(
        world.store.writes(),
        [format!(
            "frozen couple m|high over [{}]",
            couple_scope("p", "coder")
        )]
    );
}

#[tokio::test]
async fn an_uncombine_asks_on_the_member_and_says_when_it_was_not_combined() {
    let world = Build::default().world();
    let missing = world
        .stats
        .uncombine()
        .execute(&Actor::LocalOperator, "ui", uncombine(PIPELINE, "", "a"))
        .await;
    assert!(
        matches!(missing, Err(UncombineError::NotCombined)),
        "{missing:?}"
    );
    let asked = world.host.asked.lock().unwrap().clone();
    assert_eq!(asked[0].resource, ResourceKind::StatsAbsorption);
    assert_eq!(asked[0].verb, Verb::Delete);
    assert_eq!(asked[0].target.as_deref(), Some("a"));
}

#[tokio::test]
async fn a_refused_uncombine_leaves_the_absorption() {
    let world = Build {
        host: Host::refusing(),
        store: Store::holding(vec![row(PIPELINE, "", "a", "b")]),
        ..Build::default()
    }
    .world();
    let refused = world
        .stats
        .uncombine()
        .execute(&Actor::LocalOperator, "ui", uncombine(PIPELINE, "", "a"))
        .await;
    assert!(
        matches!(refused, Err(UncombineError::Refused(_))),
        "{refused:?}"
    );
    assert!(world.store.writes().is_empty());
}

#[tokio::test]
async fn a_pipeline_taken_out_releases_the_nodes_only_it_ran() {
    let mut build = Build {
        store: Store::holding(vec![
            row(PIPELINE, "", "a", "b"),
            row(NODE, "b", "review", "code-review"),
            row(NODE, "b", "shared", "shared-new"),
        ]),
        ..Build::default()
    };
    build.events.ran.insert(
        "a".into(),
        ["review".to_string(), "shared".to_string()].into(),
    );
    build.events.ran.insert(
        "b".into(),
        ["code-review".to_string(), "shared".to_string()].into(),
    );
    let world = build.world();
    world
        .stats
        .uncombine()
        .execute(&Actor::LocalOperator, "ui", uncombine(PIPELINE, "", "a"))
        .await
        .unwrap();
    assert_eq!(
        world.store.writes(),
        ["remove pipeline [] a, released [b] review"]
    );
    let audit = world.audit.0.lock().unwrap();
    assert_eq!(audit[0].action, "stats_absorption.uncombine");
}

#[tokio::test]
async fn a_rename_is_a_transition_that_asks_nothing() {
    let world = Build {
        host: Host::refusing(),
        store: Store::holding(vec![row(PIPELINE, "", "old", "keeper")]),
        ..Build::default()
    }
    .world();
    let named = |key: &str| Named {
        key: key.into(),
        name: key.into(),
    };
    world
        .stats
        .record_rename()
        .execute(&named("old"), &named("new"))
        .await
        .unwrap();
    // The old key was absorbed: the new one joins its absorbent.
    assert_eq!(
        world.store.writes(),
        ["combine pipeline [] keeper <- new (rename)"]
    );
    assert!(world.host.asked.lock().unwrap().is_empty());
}
