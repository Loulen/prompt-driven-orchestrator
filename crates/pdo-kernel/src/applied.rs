//! `OperationApplied` (D20): after each authorized and successful mutation, the
//! extensions observe who ran which Operation, on which entity.
//!
//! A mutation is authorized in its use case, but only its primary adapter
//! knows whether it succeeded. So a primary adapter runs the gesture in a
//! [`scope`]: every Operation the Autorisation d'opération allows inside it is
//! recorded (with the entity a creation makes, when [`stamp`] names it), and
//! the adapter hands the record to [`publish`] once the gesture succeeded. The
//! `identify` middleware does it for every HTTP request, a Trigger's cron tick
//! for its fire.
//!
//! An extension's own mutations take another path: it tells its
//! [`Host`](pdo_extension_api::Host) itself once one succeeded.
//!
//! [`stamp`]: crate::attributes::stamp

use std::future::Future;
use std::sync::Mutex;

pub use pdo_extension_api::OperationApplied;
use pdo_extension_api::{EntityRef, Verb};

use crate::extension_host::ExtensionHost;
use crate::{Actor, Operation};

tokio::task_local! {
    static RECORD: Mutex<Vec<OperationApplied>>;
}

/// Run `gesture`, recording the Operations authorized while it runs.
pub async fn scope<F: Future>(gesture: F) -> (F::Output, Vec<OperationApplied>) {
    RECORD
        .scope(Mutex::new(Vec::new()), async {
            let output = gesture.await;
            let record = RECORD
                .try_with(|record| std::mem::take(&mut *record.lock().unwrap()))
                .unwrap_or_default();
            (output, record)
        })
        .await
}

/// Record an allowed Operation, when a scope is open.
pub(crate) fn allowed(actor: &Actor, operation: &Operation) {
    let entity = operation
        .target
        .as_ref()
        .map(|id| EntityRef::new(operation.resource.clone(), id.clone()));
    let _ = RECORD.try_with(|record| {
        record.lock().unwrap().push(OperationApplied::new(
            actor.clone(),
            operation.clone(),
            entity,
        ))
    });
}

/// Name the entity a creation made: the latest creation of its kind recorded
/// without an entity gets it.
pub(crate) fn created(entity: &EntityRef) {
    let _ = RECORD.try_with(|record| {
        let mut record = record.lock().unwrap();
        if let Some(creation) = record.iter_mut().rev().find(|applied| {
            applied.entity.is_none()
                && applied.operation.resource == entity.kind
                && applied.operation.verb == Verb::Create
        }) {
            creation.entity = Some(entity.clone());
        }
    });
}

/// Hand the Operations of a successful gesture to the extensions (and to `/ws`
/// once an extension is loaded, [`ExtensionHost::announce`]), and drop the
/// attributes of the entities it deleted.
pub fn publish(host: &dyn ExtensionHost, record: Vec<OperationApplied>) {
    for applied in record {
        if applied.operation.verb == Verb::Delete {
            if let Some(entity) = applied.entity.clone() {
                let store = host.attributes();
                if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                    runtime.spawn(async move {
                        if let Err(e) = store.forget(&entity).await {
                            tracing::warn!(
                                "failed to drop the attributes of {:?} {}: {e}",
                                entity.kind,
                                entity.id
                            );
                        }
                    });
                }
            }
        }
        host.announce(applied.to_event());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ResourceKind, Verb};

    #[tokio::test]
    async fn a_scope_records_what_is_allowed_and_names_what_is_created() {
        let ((), record) = scope(async {
            allowed(
                &Actor::LocalOperator,
                &Operation::new(ResourceKind::Project, Verb::Create),
            );
            created(&EntityRef::new(ResourceKind::Project, "p1"));
            allowed(
                &Actor::LocalOperator,
                &Operation::new(ResourceKind::Run, Verb::Delete).on("r1"),
            );
        })
        .await;
        assert_eq!(record.len(), 2);
        assert_eq!(
            record[0].entity,
            Some(EntityRef::new(ResourceKind::Project, "p1"))
        );
        assert_eq!(
            record[1].entity,
            Some(EntityRef::new(ResourceKind::Run, "r1"))
        );
    }

    #[tokio::test]
    async fn outside_a_scope_nothing_is_recorded() {
        allowed(
            &Actor::LocalOperator,
            &Operation::new(ResourceKind::Project, Verb::Create),
        );
        let ((), record) = scope(async {}).await;
        assert!(record.is_empty());
    }
}
