//! API 0.8: the last editor of a Trigger, stored as the Actor's JSON.

use pdo_kernel::Actor;
use pdo_trigger::adapters::sqlite::*;
use pdo_trigger::domain::trigger::UpdateTrigger;

use super::{sample, test_db};

fn alice() -> Actor {
    Actor::Identified {
        namespace: "iam".into(),
        id: "alice".into(),
    }
}

#[tokio::test]
async fn the_editor_of_a_creation_round_trips() {
    let db = test_db().await;
    let mut new = sample("nightly", "0 9 * * *");
    new.last_editor = Some(alice());
    let created = create(&db, new).await.unwrap();
    assert_eq!(created.last_editor, Some(alice()));
    assert_eq!(
        get(&db, &created.id).await.unwrap().unwrap().last_editor,
        Some(alice())
    );
}

#[tokio::test]
async fn an_edit_replaces_the_editor_and_an_edit_without_one_keeps_it() {
    let db = test_db().await;
    let created = create(&db, sample("nightly", "0 9 * * *")).await.unwrap();
    assert_eq!(created.last_editor, None, "none recorded");

    update(
        &db,
        &created.id,
        UpdateTrigger {
            last_editor: Some(Actor::LocalOperator),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        get(&db, &created.id).await.unwrap().unwrap().last_editor,
        Some(Actor::LocalOperator)
    );

    update(
        &db,
        &created.id,
        UpdateTrigger {
            name: Some("renamed".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        get(&db, &created.id).await.unwrap().unwrap().last_editor,
        Some(Actor::LocalOperator)
    );
}

#[tokio::test]
async fn init_adds_last_editor_to_a_legacy_table_and_an_unreadable_value_is_none() {
    let db = test_db().await;
    let created = create(&db, sample("legacy", "0 9 * * *")).await.unwrap();
    sqlx::query("UPDATE triggers SET last_editor = 'not json' WHERE id = ?")
        .bind(&created.id)
        .execute(&db)
        .await
        .unwrap();
    init(&db).await.unwrap(); // idempotent on a table that already has it
    assert_eq!(
        get(&db, &created.id).await.unwrap().unwrap().last_editor,
        None
    );
}
