//! The caller's Apparence: each Actor reads what it wrote, never another's; a
//! wrong document is refused and leaves the stored one alone; no Operation is
//! asked.

use pdo_kernel::Actor;
use pdo_settings::application::SettingsError;

use crate::fakes::world;

fn identified(id: &str) -> Actor {
    Actor::Identified {
        namespace: "test".into(),
        id: id.into(),
    }
}

#[tokio::test]
async fn an_actor_reads_back_its_own_appearance_only() {
    let w = world();
    let (alice, bob) = (identified("alice"), identified("bob"));
    assert_eq!(w.settings.appearance(&alice).await.unwrap(), None);

    let kept = w
        .settings
        .set_appearance(
            &alice,
            br#"{"version":1,"theme":"classic","dark_mode":false}"#,
        )
        .await
        .unwrap();
    assert_eq!(kept.theme.as_deref(), Some("classic"));
    assert_eq!(w.settings.appearance(&alice).await.unwrap(), Some(kept));
    assert_eq!(w.settings.appearance(&bob).await.unwrap(), None);
    assert_eq!(
        w.settings.appearance(&Actor::LocalOperator).await.unwrap(),
        None
    );
    assert!(w.asked().is_empty(), "{:?}", w.asked());
}

#[tokio::test]
async fn a_wrong_document_is_refused_and_stores_nothing() {
    let w = world();
    let me = Actor::LocalOperator;
    w.settings
        .set_appearance(&me, br#"{"version":1,"compact_nodes":true}"#)
        .await
        .unwrap();
    let err = w
        .settings
        .set_appearance(&me, br#"{"version":1,"compact_nodes":"on"}"#)
        .await
        .unwrap_err();
    assert!(matches!(err, SettingsError::Invalid(_)), "{err:?}");
    let err = w
        .settings
        .set_appearance(&me, &vec![b' '; 64 * 1024])
        .await
        .unwrap_err();
    assert!(matches!(err, SettingsError::TooLarge(_)), "{err:?}");
    let stored = w.settings.appearance(&me).await.unwrap().unwrap();
    assert_eq!(stored.compact_nodes, Some(true));
    assert_eq!(w.appearances.documents.lock().unwrap().len(), 1);
}
