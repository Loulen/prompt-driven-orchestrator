//! Stage 3 (ADR-0080): the agentic profiles over a real SQLite (the legacy
//! crate's `agent_profile` tests of the store, moved; the rules of a create or
//! an edit are the use cases', tested at stage 2).

use pdo_harness::registry::CLAUDE;
use pdo_settings::adapters::sqlite::agent_profiles::{
    create, delete, find_by_name, get, list, replace, snapshot,
};
use pdo_settings::adapters::sqlite::init;
use pdo_settings::domain::agent_profile::{ProfileDraft, DEFAULT_PROFILE_ID, DEFAULT_PROFILE_NAME};
use sqlx::SqlitePool;

async fn mem_db() -> SqlitePool {
    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    init(&db).await.unwrap();
    db
}

fn draft(name: &str, harness: &str, model: Option<&str>) -> ProfileDraft {
    ProfileDraft::new(name, harness, model, None).unwrap()
}

#[tokio::test]
async fn init_is_idempotent_and_seeds_default_exactly_once() {
    let db = mem_db().await;
    init(&db).await.unwrap();
    let all = list(&db).await.unwrap();
    assert_eq!(all.len(), 1, "Default must be seeded exactly once");
    let d = &all[0];
    assert_eq!(d.id, DEFAULT_PROFILE_ID);
    assert_eq!(d.name, DEFAULT_PROFILE_NAME);
    assert_eq!(d.harness, CLAUDE);
    assert_eq!((d.model.as_deref(), d.effort.as_deref()), (None, None));
}

#[tokio::test]
async fn default_profile_is_immediately_resolvable() {
    // AC10: Default toujours disponible dès l'instanciation.
    let db = mem_db().await;
    let d = get(&db, DEFAULT_PROFILE_ID).await.unwrap().unwrap();
    assert_eq!(d.combo().harness, CLAUDE);
}

#[tokio::test]
async fn a_created_profile_is_stored_under_a_fresh_id() {
    let db = mem_db().await;
    let p = create(&db, draft("Fast Reviewer", "claude", Some("opus")))
        .await
        .unwrap();
    assert!(p.id.starts_with("agp-"), "{}", p.id);
    assert_eq!(get(&db, &p.id).await.unwrap().unwrap(), p);
}

#[tokio::test]
async fn a_replace_keeps_the_id_and_the_creation_time() {
    // ADR-0057 ¶2: the identity does not depend on the name.
    let db = mem_db().await;
    let p = create(&db, draft("Old Name", "claude", None))
        .await
        .unwrap();
    let updated = replace(&db, &p.id, draft("New Name", "opencode", Some("gpt")))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (updated.id.as_str(), updated.name.as_str()),
        (p.id.as_str(), "New Name")
    );
    assert_eq!(updated.created_at, p.created_at);
    assert_eq!(updated.model.as_deref(), Some("gpt"));
    assert_eq!(
        replace(&db, "agp-nope", draft("X", "claude", None))
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn the_unique_index_refuses_a_case_insensitive_clash() {
    // The last line of defence behind the use case's named refusal.
    let db = mem_db().await;
    create(&db, draft("Reviewer", "claude", None))
        .await
        .unwrap();
    assert!(create(&db, draft("REVIEWER", "claude", None))
        .await
        .is_err());
}

#[tokio::test]
async fn a_named_profile_can_be_deleted_and_an_unknown_id_is_a_noop() {
    let db = mem_db().await;
    let p = create(&db, draft("Temp", "claude", None)).await.unwrap();
    assert!(delete(&db, &p.id).await.unwrap());
    assert!(get(&db, &p.id).await.unwrap().is_none());
    assert!(!delete(&db, "agp-nope").await.unwrap());
}

#[tokio::test]
async fn snapshot_is_one_atomic_read_of_every_profile() {
    // ADR-0057 ¶4: the spawn seam reads one full revision in one shot.
    let db = mem_db().await;
    let a = create(&db, draft("Alpha", "claude", Some("opus")))
        .await
        .unwrap();
    let b = create(&db, draft("Bravo", "opencode", None)).await.unwrap();
    let snap = snapshot(&db).await.unwrap();
    assert_eq!(snap.len(), 3, "Default + Alpha + Bravo");
    assert_eq!(snap[DEFAULT_PROFILE_ID].harness, CLAUDE);
    assert_eq!(snap[&a.id].model.as_deref(), Some("opus"));
    assert_eq!(snap[&b.id].harness, "opencode");
    assert_eq!(snap[&b.id].model, None);
}

#[tokio::test]
async fn list_orders_default_first_then_creation_order() {
    let db = mem_db().await;
    let a = create(&db, draft("Alpha", "claude", None)).await.unwrap();
    let b = create(&db, draft("Bravo", "claude", None)).await.unwrap();
    let ids: Vec<String> = list(&db).await.unwrap().into_iter().map(|p| p.id).collect();
    assert_eq!(ids, vec![DEFAULT_PROFILE_ID.to_string(), a.id, b.id]);
}

#[tokio::test]
async fn find_by_name_matches_regardless_of_case_and_whitespace() {
    let db = mem_db().await;
    let p = create(&db, draft("Fast Reviewer", "claude", None))
        .await
        .unwrap();
    let found = find_by_name(&db, "  fast REVIEWER  ").await.unwrap();
    assert_eq!(found.unwrap().id, p.id);
    assert!(find_by_name(&db, "nope").await.unwrap().is_none());
}
