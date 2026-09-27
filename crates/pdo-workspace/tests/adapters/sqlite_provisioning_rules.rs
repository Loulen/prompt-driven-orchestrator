//! Provisioning rules over a real SQLite database.

use pdo_workspace::adapters::sqlite::{init, SqliteProvisioningRules};
use pdo_workspace::application::ports::ProvisioningRulesRepository;
use pdo_workspace::domain::provisioning::{ProvisioningRules, ProvisioningScope};
use sqlx::SqlitePool;

fn rules(copy: &str) -> ProvisioningRules {
    ProvisioningRules {
        copy: vec![copy.into()],
        ..Default::default()
    }
}

#[tokio::test]
async fn rules_are_empty_until_saved_then_replaced_per_scope() {
    let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
    init(&db).await.unwrap();
    let store = SqliteProvisioningRules::new(db);
    let instance = ProvisioningScope::Instance;
    assert!(store.load(instance, "instance").await.unwrap().is_empty());

    store
        .save(instance, "instance", &rules(".env"))
        .await
        .unwrap();
    store
        .save(ProvisioningScope::Project, "p1", &rules("x"))
        .await
        .unwrap();
    store
        .save(instance, "instance", &rules(".envrc"))
        .await
        .unwrap();

    assert_eq!(
        store.load(instance, "instance").await.unwrap(),
        rules(".envrc")
    );
    assert_eq!(
        store.load(ProvisioningScope::Project, "p1").await.unwrap(),
        rules("x")
    );
    assert!(store
        .load(ProvisioningScope::Project, "p2")
        .await
        .unwrap()
        .is_empty());
}
