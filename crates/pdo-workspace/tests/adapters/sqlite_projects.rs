//! The Projets over a real SQLite database: schema, migrations, membership,
//! and the settings stored for the contexts downstream.

use pdo_workspace::adapters::sqlite::{containing, first_project_containing, init, SqliteProjects};
use pdo_workspace::application::ports::ProjectRepository;
use pdo_workspace::domain::project::StoredSetting;
use sqlx::SqlitePool;

async fn mem_db() -> SqlitePool {
    let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
    init(&db).await.unwrap();
    db
}

#[tokio::test]
async fn init_is_idempotent() {
    let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
    init(&db).await.unwrap();
    init(&db).await.unwrap();
    // A project created before a later init survives it.
    let store = SqliteProjects::new(db.clone());
    let p = store.create("P").await.unwrap();
    init(&db).await.unwrap();
    assert!(store.get(&p.id).await.unwrap().is_some());
}

#[tokio::test]
async fn no_row_exists_before_naming() {
    let db = mem_db().await;
    let store = SqliteProjects::new(db.clone());
    assert!(store.list().await.unwrap().is_empty());
    assert!(containing(&db, "/repos/front").await.unwrap().is_empty());
}

#[tokio::test]
async fn a_created_projet_is_named_empty_and_listed_newest_first() {
    let db = mem_db().await;
    let store = SqliteProjects::new(db);
    let a = store.create("Alpha").await.unwrap();
    assert!(a.id.starts_with("prj-"));
    assert!(a.members.is_empty() && a.harness.is_none() && a.agent_choice.is_none());
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let b = store.create("Bravo").await.unwrap();
    let names: Vec<_> = store
        .list()
        .await
        .unwrap()
        .into_iter()
        .map(|p| p.id)
        .collect();
    assert_eq!(names, vec![b.id, a.id]);
}

#[tokio::test]
async fn membership_comparison_is_verbatim_and_ordered_by_attach() {
    let db = mem_db().await;
    let store = SqliteProjects::new(db.clone());
    let a = store.create("Alpha").await.unwrap();
    store.add_member(&a.id, "/repos/front").await.unwrap();
    store.add_member(&a.id, "/repos/back").await.unwrap();
    assert_eq!(containing(&db, "/repos/front").await.unwrap().len(), 1);
    assert!(containing(&db, "/repos/front/").await.unwrap().is_empty());
    assert!(containing(&db, "/repos/FRONT").await.unwrap().is_empty());
    assert_eq!(
        store.get(&a.id).await.unwrap().unwrap().members,
        vec!["/repos/front".to_string(), "/repos/back".to_string()]
    );
}

#[tokio::test]
async fn a_path_may_be_a_member_of_several_projets() {
    let db = mem_db().await;
    let store = SqliteProjects::new(db.clone());
    let a = store.create("Alpha").await.unwrap();
    let b = store.create("Bravo").await.unwrap();
    store.add_member(&a.id, "/repos/shared").await.unwrap();
    store.add_member(&b.id, "/repos/shared").await.unwrap();
    let ids: Vec<_> = containing(&db, "/repos/shared")
        .await
        .unwrap()
        .into_iter()
        .map(|p| p.id)
        .collect();
    assert_eq!(
        ids,
        vec![a.id.clone(), b.id.clone()],
        "in the order it joined"
    );
    assert_eq!(
        first_project_containing(&db, "/repos/shared")
            .await
            .unwrap(),
        Some(a.id.clone())
    );
    // Detaching from one Projet leaves the other's membership alone.
    store.remove_member(&a.id, "/repos/shared").await.unwrap();
    let ids: Vec<_> = containing(&db, "/repos/shared")
        .await
        .unwrap()
        .into_iter()
        .map(|p| p.id)
        .collect();
    assert_eq!(ids, vec![b.id]);
}

#[tokio::test]
async fn the_default_primary_is_stored_and_cleared() {
    let db = mem_db().await;
    let store = SqliteProjects::new(db);
    let a = store.create("Alpha").await.unwrap();
    store.add_member(&a.id, "/repos/web").await.unwrap();
    store.add_member(&a.id, "/repos/api").await.unwrap();
    store
        .set_primary_repo(&a.id, Some("/repos/api"))
        .await
        .unwrap();
    let p = store.get(&a.id).await.unwrap().unwrap();
    assert_eq!(p.primary_repo.as_deref(), Some("/repos/api"));
    assert_eq!(p.default_primary(), Some("/repos/api"));
    store.set_primary_repo(&a.id, None).await.unwrap();
    let p = store.get(&a.id).await.unwrap().unwrap();
    assert_eq!(p.default_primary(), Some("/repos/web"));
}

#[tokio::test]
async fn init_migrates_one_projet_per_path_to_several() {
    // The schema before ADR-0087: `path` alone was the key.
    let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::query(
        "CREATE TABLE projects (
            id TEXT PRIMARY KEY, name TEXT NOT NULL, harness TEXT, created_at TEXT NOT NULL
        )",
    )
    .execute(&db)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TABLE project_members (
            path TEXT PRIMARY KEY, project_id TEXT NOT NULL, created_at TEXT NOT NULL
        )",
    )
    .execute(&db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO projects (id, name, created_at) VALUES ('p1', 'Alpha', 's'), ('p2', 'Bravo', 's')",
    )
    .execute(&db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO project_members (path, project_id, created_at)          VALUES ('/repos/web', 'p1', 's'), ('/repos/api', 'p1', 's')",
    )
    .execute(&db)
    .await
    .unwrap();

    init(&db).await.unwrap();
    init(&db).await.unwrap();

    let store = SqliteProjects::new(db.clone());
    assert_eq!(
        store.get("p1").await.unwrap().unwrap().members,
        vec!["/repos/web".to_string(), "/repos/api".to_string()],
        "the rows and their order survive"
    );
    store.add_member("p2", "/repos/web").await.unwrap();
    assert_eq!(containing(&db, "/repos/web").await.unwrap().len(), 2);
}

#[tokio::test]
async fn settings_of_other_contexts_are_stored_verbatim_and_cleared_to_null() {
    let db = mem_db().await;
    let store = SqliteProjects::new(db);
    let a = store.create("Alpha").await.unwrap();
    let choice = StoredSetting(r#"{"mode":"profile","profile_id":"reviewer"}"#.into());
    let skills = StoredSetting(r#"[{"id":"tdd","name":"TDD"}]"#.into());
    store
        .set_agent_choice(&a.id, Some(choice.clone()))
        .await
        .unwrap();
    store.set_skills(&a.id, Some(skills.clone())).await.unwrap();
    let p = store.get(&a.id).await.unwrap().unwrap();
    assert_eq!(p.agent_choice, Some(choice));
    assert_eq!(p.skills, Some(skills));

    store.set_agent_choice(&a.id, None).await.unwrap();
    store.set_skills(&a.id, None).await.unwrap();
    let p = store.get(&a.id).await.unwrap().unwrap();
    assert_eq!((p.agent_choice, p.skills), (None, None));
}

#[tokio::test]
async fn init_migrates_pre_agent_choice_schema() {
    let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::query(
        "CREATE TABLE projects (
            id         TEXT PRIMARY KEY,
            name       TEXT NOT NULL,
            harness    TEXT,
            created_at TEXT NOT NULL
        )",
    )
    .execute(&db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO projects (id, name, harness, created_at) \
         VALUES ('prj-1', 'Alpha', 'claude', 'seed')",
    )
    .execute(&db)
    .await
    .unwrap();

    init(&db).await.unwrap();
    init(&db).await.unwrap();

    let store = SqliteProjects::new(db);
    let p = store.get("prj-1").await.unwrap().unwrap();
    assert_eq!(
        p.harness.as_deref(),
        Some("claude"),
        "pre-existing survives"
    );
    assert_eq!(p.agent_choice, None, "new column defaults to NULL");
    assert_eq!(p.auto_fail, None);
    assert_eq!(p.skills, None);

    let choice = StoredSetting(r#"{"mode":"inherit"}"#.into());
    store
        .set_agent_choice("prj-1", Some(choice.clone()))
        .await
        .unwrap();
    assert_eq!(
        store.get("prj-1").await.unwrap().unwrap().agent_choice,
        Some(choice)
    );
}

#[tokio::test]
async fn rename_and_delete_free_the_members() {
    let db = mem_db().await;
    let store = SqliteProjects::new(db.clone());
    let a = store.create("Alpha").await.unwrap();
    store.add_member(&a.id, "/repos/front").await.unwrap();

    store.rename(&a.id, "Renamed").await.unwrap();
    assert_eq!(store.get(&a.id).await.unwrap().unwrap().name, "Renamed");

    assert!(store.delete(&a.id).await.unwrap());
    assert!(
        !store.delete(&a.id).await.unwrap(),
        "a second delete finds none"
    );
    assert!(store.get(&a.id).await.unwrap().is_none());
    assert!(containing(&db, "/repos/front").await.unwrap().is_empty());
}

#[tokio::test]
async fn a_deleted_projet_keeps_its_id_and_last_name() {
    let db = mem_db().await;
    let store = SqliteProjects::new(db.clone());
    assert!(store.deleted().await.unwrap().is_empty());
    let a = store.create("Alpha").await.unwrap();
    let b = store.create("Beta").await.unwrap();
    store.rename(&a.id, "Renamed").await.unwrap();

    store.delete(&a.id).await.unwrap();
    store.delete(&b.id).await.unwrap();
    init(&db).await.unwrap();

    let deleted: Vec<_> = store
        .deleted()
        .await
        .unwrap()
        .into_iter()
        .map(|p| (p.id, p.name))
        .collect();
    assert_eq!(
        deleted,
        [(b.id, "Beta".to_string()), (a.id, "Renamed".to_string())]
    );
}

#[tokio::test]
async fn remove_member_frees_the_path() {
    let db = mem_db().await;
    let store = SqliteProjects::new(db.clone());
    let a = store.create("Alpha").await.unwrap();
    store.add_member(&a.id, "/repos/front").await.unwrap();
    store.remove_member(&a.id, "/repos/front").await.unwrap();
    assert!(containing(&db, "/repos/front").await.unwrap().is_empty());
    store.remove_member(&a.id, "/repos/front").await.unwrap();
}
