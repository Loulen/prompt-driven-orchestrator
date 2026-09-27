//! The SQLite index of skills, folders and scanned sources.

use pdo_skill::adapters::sqlite::{
    init, snapshot_names, SqliteFolderIndex, SqliteSkillIndex, SqliteSourceHistory,
};
use pdo_skill::application::ports::{FolderIndex, ScannedSource, SkillIndex, SourceHistory};
use pdo_skill::domain::skill::{FolderProvenance, Provenance, Skill, SkillFolder};
use sqlx::SqlitePool;

async fn db() -> SqlitePool {
    let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
    init(&db).await.unwrap();
    init(&db).await.unwrap();
    db
}

fn skill(id: &str, name: &str, folder: Option<&str>) -> Skill {
    Skill {
        id: id.into(),
        name: name.into(),
        description: "d".into(),
        folder_id: folder.map(str::to_string),
        locked: false,
        source: None,
        created_at: format!("2026-01-01T00:00:00.00{}Z", id.len() % 10),
        updated_at: "2026-01-01T00:00:00.000Z".into(),
    }
}

fn folder(id: &str, name: &str, parent: Option<&str>) -> SkillFolder {
    SkillFolder {
        id: id.into(),
        name: name.into(),
        parent_id: parent.map(str::to_string),
        source: None,
        created_at: "2026-01-01T00:00:00.000Z".into(),
        updated_at: "2026-01-01T00:00:00.000Z".into(),
    }
}

#[tokio::test]
async fn skills_round_trip_with_their_provenance_and_lock() {
    let db = db().await;
    let index = SqliteSkillIndex::new(db.clone());
    let mut tdd = skill("s1", "tdd", None);
    tdd.source = Some(Provenance {
        url: "https://github.com/o/r".into(),
        git_ref: Some("main".into()),
        commit: Some("c1".into()),
        path: "skills/tdd".into(),
    });
    index.insert(&tdd).await.unwrap();
    index
        .insert(&skill("pdo-orchestrate", "pdo-orchestrate", None))
        .await
        .unwrap();
    assert_eq!(index.get("s1").await.unwrap().unwrap(), tdd);
    assert!(index.get("pdo-orchestrate").await.unwrap().unwrap().locked);
    let listed: Vec<_> = index
        .list()
        .await
        .unwrap()
        .into_iter()
        .map(|s| s.name)
        .collect();
    assert_eq!(listed, vec!["pdo-orchestrate", "tdd"]);
    assert_eq!(
        index.find_by_name("  TDD ").await.unwrap().unwrap().id,
        "s1"
    );
    assert_eq!(snapshot_names(&db).await.unwrap().len(), 2);
}

#[tokio::test]
async fn the_label_index_refuses_a_case_insensitive_duplicate() {
    let db = db().await;
    let index = SqliteSkillIndex::new(db);
    index.insert(&skill("s1", "tdd", None)).await.unwrap();
    assert!(index.insert(&skill("s2", "TDD", None)).await.is_err());
}

#[tokio::test]
async fn save_rewrites_the_row_and_set_provenance_keeps_updated_at() {
    let db = db().await;
    let index = SqliteSkillIndex::new(db);
    let mut tdd = skill("s1", "tdd", None);
    index.insert(&tdd).await.unwrap();
    tdd.name = "tdd-strict".into();
    tdd.folder_id = Some("skf-1".into());
    tdd.updated_at = "2026-02-02T00:00:00.000Z".into();
    index.save(&tdd).await.unwrap();
    assert_eq!(index.get("s1").await.unwrap().unwrap(), tdd);
    let provenance = Provenance {
        url: "u".into(),
        git_ref: None,
        commit: Some("c2".into()),
        path: "p".into(),
    };
    index.set_provenance("s1", &provenance).await.unwrap();
    let row = index.get("s1").await.unwrap().unwrap();
    assert_eq!(row.source, Some(provenance));
    assert_eq!(row.updated_at, "2026-02-02T00:00:00.000Z");
    index
        .refile("skf-1", None, "2026-03-03T00:00:00.000Z")
        .await
        .unwrap();
    assert_eq!(index.get("s1").await.unwrap().unwrap().folder_id, None);
    assert!(index.remove("s1").await.unwrap());
    assert!(!index.remove("s1").await.unwrap());
}

#[tokio::test]
async fn folders_round_trip_reparent_and_carry_a_source() {
    let db = db().await;
    let folders = SqliteFolderIndex::new(db);
    folders.insert(&folder("a", "PDO", None)).await.unwrap();
    folders
        .insert(&folder("b", "java", Some("a")))
        .await
        .unwrap();
    assert_eq!(
        folders.root_named(" pdo ").await.unwrap().as_deref(),
        Some("a")
    );
    assert_eq!(folders.root_named("java").await.unwrap(), None);
    let provenance = FolderProvenance {
        url: "u".into(),
        git_ref: Some("main".into()),
        commit: Some("c1".into()),
        path: "skills".into(),
        imported_at: "t".into(),
        found: 3,
        invalid: 1,
    };
    folders
        .set_provenance("b", &provenance, "now")
        .await
        .unwrap();
    let b = folders.get("b").await.unwrap().unwrap();
    assert_eq!(b.source, Some(provenance));
    assert_eq!(b.updated_at, "now");
    folders.reparent("a", None, "later").await.unwrap();
    assert_eq!(folders.get("b").await.unwrap().unwrap().parent_id, None);
    let mut renamed = folder("a", "Acme", None);
    renamed.updated_at = "later".into();
    folders.save(&renamed).await.unwrap();
    let names: Vec<_> = folders
        .list()
        .await
        .unwrap()
        .into_iter()
        .map(|f| f.name)
        .collect();
    assert_eq!(names, vec!["Acme", "java"]);
    folders.remove("a").await.unwrap();
    assert!(folders.get("a").await.unwrap().is_none());
}

#[tokio::test]
async fn scanned_sources_are_remembered_once_most_recent_first() {
    let db = db().await;
    let history = SqliteSourceHistory::new(db);
    let source = |url: &str, at: &str| ScannedSource {
        url: url.into(),
        git_ref: String::new(),
        path: String::new(),
        last_used_at: at.into(),
    };
    history.remember(&source("a", "1")).await.unwrap();
    history.remember(&source("b", "2")).await.unwrap();
    history.remember(&source("a", "3")).await.unwrap();
    let recent: Vec<_> = history
        .recent(12)
        .await
        .unwrap()
        .into_iter()
        .map(|s| s.url)
        .collect();
    assert_eq!(recent, vec!["a", "b"]);
    assert_eq!(history.recent(1).await.unwrap().len(), 1);
}
