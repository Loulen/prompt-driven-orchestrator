//! The extension data of a creation (API 0.9): which ids are known, what each
//! extension says of its value, and the port through which an extension's
//! [`Host`](pdo_extension_api::Host) creates a Run.

use std::panic::{catch_unwind, AssertUnwindSafe};

use pdo_extension_api::{ExtensionData, NewRun, ResourceKind, RunCreation};
use serde_json::{json, Value};

use super::{panic_message, Extensions};
use crate::Actor;

/// Why a creation's extension data is refused: `400`, nothing created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreationRefusal {
    /// The data names an extension that is not installed, like an unknown
    /// field.
    UnknownExtension { id: String, installed: Vec<String> },
    /// The extension refused its value, with this reason.
    Refused { extension: String, reason: String },
}

impl CreationRefusal {
    /// The JSON body of the `400`: `error` names the key (or the extension
    /// and its reason); a refusal also carries `extension` and `reason`.
    pub fn body(&self) -> Value {
        match self {
            CreationRefusal::UnknownExtension { id, installed } => {
                let known = if installed.is_empty() {
                    "no extension is installed".to_string()
                } else {
                    format!("installed extensions: {}", installed.join(", "))
                };
                json!({ "error": format!("unknown extension `{id}` in `extensions`; {known}") })
            }
            CreationRefusal::Refused { extension, reason } => json!({
                "error": format!("extension `{extension}` refused the creation: {reason}"),
                "extension": extension,
                "reason": reason,
            }),
        }
    }
}

/// The first id of `data` that `installed` does not hold, as a refusal.
pub(super) fn unknown_extension(
    data: &ExtensionData,
    installed: &[String],
) -> Result<(), CreationRefusal> {
    match data.keys().find(|id| !installed.contains(id)) {
        Some(id) => Err(CreationRefusal::UnknownExtension {
            id: id.clone(),
            installed: installed.to_vec(),
        }),
        None => Ok(()),
    }
}

/// The ids, then each present extension's `validate_create`, in registration
/// order; the first refusal wins. A panic refuses.
pub(super) fn validate(
    extensions: &Extensions,
    actor: &Actor,
    kind: &ResourceKind,
    data: &ExtensionData,
) -> Result<(), CreationRefusal> {
    let installed: Vec<String> = extensions
        .iter()
        .map(|(manifest, _)| manifest.id.clone())
        .collect();
    unknown_extension(data, &installed)?;
    for (manifest, extension) in extensions.iter() {
        let Some(value) = data.get(&manifest.id) else {
            continue;
        };
        let answer = catch_unwind(AssertUnwindSafe(|| {
            extension.validate_create(actor, kind, value)
        }));
        let refusal = match answer {
            Ok(Ok(())) => continue,
            Ok(Err(reason)) => reason,
            Err(panic) => {
                tracing::error!(
                    "extension `{}` panicked in validate_create ({})",
                    manifest.id,
                    panic_message(panic.as_ref())
                );
                format!("extension `{}` failed", manifest.id)
            }
        };
        return Err(CreationRefusal::Refused {
            extension: manifest.id.clone(),
            reason: refusal,
        });
    }
    Ok(())
}

/// What creates a Run for an extension's [`Host::create_run`](pdo_extension_api::Host::create_run)
/// (a port: the Runs of the run context in the daemon), as `POST /runs`
/// creates one for `actor`.
pub trait RunCreator: Send + Sync {
    fn create_run(&self, actor: Actor, run: NewRun) -> RunCreation;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension_host::ExtensionHost;
    use pdo_extension_api::{Extension, Manifest, RunNotCreated};
    use std::sync::{Arc, Mutex};

    /// Refuses a value that is not an object, records what it checked.
    #[derive(Default)]
    struct Picky(Arc<Mutex<Vec<Value>>>);
    impl Extension for Picky {
        fn manifest(&self) -> Manifest {
            Manifest::new("picky", "Picky", "0.0.0")
        }
        fn validate_create(
            &self,
            _actor: &Actor,
            _kind: &ResourceKind,
            value: &Value,
        ) -> Result<(), String> {
            self.0.lock().unwrap().push(value.clone());
            if value.is_object() {
                Ok(())
            } else {
                Err("an object, please".into())
            }
        }
    }

    struct Panicky;
    impl Extension for Panicky {
        fn manifest(&self) -> Manifest {
            Manifest::new("panicky", "Panicky", "0.0.0")
        }
        fn validate_create(&self, _: &Actor, _: &ResourceKind, _: &Value) -> Result<(), String> {
            panic!("boom")
        }
    }

    fn data(pairs: &[(&str, Value)]) -> ExtensionData {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn an_unknown_id_is_refused_naming_it_and_the_installed_ones() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let host = Extensions::register(vec![Box::new(Picky(seen.clone()))]).unwrap();
        let refusal = host
            .validate_create(
                &Actor::LocalOperator,
                &ResourceKind::Run,
                &data(&[("picky", json!({})), ("ghost", json!(1))]),
            )
            .unwrap_err();
        assert_eq!(
            refusal.body(),
            json!({ "error": "unknown extension `ghost` in `extensions`; installed extensions: picky" })
        );
        assert!(
            seen.lock().unwrap().is_empty(),
            "nothing asked before the ids pass"
        );
        assert_eq!(
            crate::extension_host::NoExtensions
                .validate_create(
                    &Actor::LocalOperator,
                    &ResourceKind::Run,
                    &data(&[("x", json!(1))])
                )
                .unwrap_err()
                .body()["error"],
            "unknown extension `x` in `extensions`; no extension is installed"
        );
    }

    #[test]
    fn only_the_present_extensions_check_their_value() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let host =
            Extensions::register(vec![Box::new(Picky(seen.clone())), Box::new(Panicky)]).unwrap();
        let actor = Actor::LocalOperator;
        assert_eq!(
            host.validate_create(&actor, &ResourceKind::Run, &ExtensionData::new()),
            Ok(())
        );
        assert_eq!(
            host.validate_create(
                &actor,
                &ResourceKind::Run,
                &data(&[("picky", json!({"k": 1}))])
            ),
            Ok(()),
            "Panicky has no value: never asked"
        );
        let refusal = host
            .validate_create(
                &actor,
                &ResourceKind::Run,
                &data(&[("picky", json!("flat"))]),
            )
            .unwrap_err();
        assert_eq!(
            refusal.body(),
            json!({
                "error": "extension `picky` refused the creation: an object, please",
                "extension": "picky",
                "reason": "an object, please",
            })
        );
        assert_eq!(
            host.validate_create(&actor, &ResourceKind::Run, &data(&[("panicky", json!(1))])),
            Err(CreationRefusal::Refused {
                extension: "panicky".into(),
                reason: "extension `panicky` failed".into(),
            }),
            "a panic refuses"
        );
        assert_eq!(*seen.lock().unwrap(), vec![json!({"k": 1}), json!("flat")]);
    }

    #[tokio::test]
    async fn a_host_creates_runs_once_the_daemon_plugs_its_runs() {
        struct Echo;
        impl RunCreator for Echo {
            fn create_run(&self, actor: Actor, run: NewRun) -> RunCreation {
                Box::pin(async move { Ok(format!("{}:{}", actor.kind(), run.pipeline)) })
            }
        }
        let extensions = Extensions::register(vec![Box::new(Picky::default())]).unwrap();
        let host = extensions.host_of("picky");
        let before = host
            .create_run(&Actor::LocalOperator, NewRun::new("relay", ""))
            .await
            .unwrap_err();
        assert_eq!(before.status, 503);
        extensions.serve_runs(Arc::new(Echo));
        assert_eq!(
            host.create_run(&Actor::LocalOperator, NewRun::new("relay", ""))
                .await,
            Ok::<_, RunNotCreated>("local_operator:relay".to_string())
        );
    }
}
