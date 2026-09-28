//! The staging profiles as the operator manages them: the read side, and the one
//! service every profile use case hangs off.

use std::path::PathBuf;
use std::sync::Arc;

use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::ports::{HostHome, ProfileReferents, Referents, SandboxProfileRepository, StoreError};
use crate::domain::mode::SandboxMode;
use crate::domain::profile::{
    profile_names, resolve_profile, ResolvedProfile, FULL_PROFILE, MINIMAL_PROFILE,
};

/// Why a profile use case did not do what it was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileError {
    /// The request is malformed; the message names the offending value (400).
    Invalid(String),
    /// No such profile, or no row to delete (404).
    NotFound(String),
    /// An extension refused the Operation (403).
    Denied(Denied),
    /// The store failed (500).
    Store(String),
}

impl From<Denied> for ProfileError {
    fn from(denied: Denied) -> Self {
        ProfileError::Denied(denied)
    }
}

impl From<StoreError> for ProfileError {
    fn from(e: StoreError) -> Self {
        ProfileError::Store(e.0)
    }
}

/// The staging profiles of the instance, and what reads and changes them.
pub struct SandboxProfiles {
    pub(crate) store: Arc<dyn SandboxProfileRepository>,
    pub(crate) host: Arc<dyn ExtensionHost>,
    pub(crate) home: Arc<dyn HostHome>,
    pub(crate) referents: Arc<dyn ProfileReferents>,
}

impl SandboxProfiles {
    pub fn new(
        store: Arc<dyn SandboxProfileRepository>,
        host: Arc<dyn ExtensionHost>,
        home: Arc<dyn HostHome>,
        referents: Arc<dyn ProfileReferents>,
    ) -> Self {
        Self {
            store,
            host,
            home,
            referents,
        }
    }

    /// The daemon's home, the root every entry is relative to.
    pub fn home(&self) -> Option<PathBuf> {
        self.home.home()
    }

    /// Ask the extensions whether `actor` may `verb` a profile: the one Autorisation
    /// d'opération of the context. `target` is the profile's name, `None` for a
    /// creation.
    pub(crate) fn authorize(
        &self,
        actor: &Actor,
        verb: Verb,
        target: Option<&str>,
    ) -> Result<(), Denied> {
        let operation = Operation::new(ResourceKind::SandboxProfile, verb);
        let operation = match target {
            Some(name) => operation.on(name),
            None => operation,
        };
        authorize(self.host.as_ref(), actor, &operation)
    }

    /// See [`resolve`].
    pub async fn resolve(&self, name: &str) -> Result<Option<ResolvedProfile>, StoreError> {
        resolve(self.store.as_ref(), name).await
    }

    /// See [`exists`].
    pub async fn exists(&self, name: &str) -> Result<bool, StoreError> {
        exists(self.store.as_ref(), name).await
    }

    /// See [`names`].
    pub async fn names(&self) -> Result<Vec<(String, bool)>, StoreError> {
        names(self.store.as_ref()).await
    }

    /// Who still points at `name`.
    pub async fn referents(&self, name: &str) -> Referents {
        self.referents.referents(name).await
    }
}

/// Resolve `name` to its effective entry list, or `None` when no such profile exists —
/// the ONE existence oracle (ADR-0031 §7): every edge turns `None` into a hard failure
/// (400 at create, a red Trigger fire record, `RunFailed` at boot recovery) so an
/// unknown name can never fall back silently to less isolation.
pub async fn resolve(
    store: &dyn SandboxProfileRepository,
    name: &str,
) -> Result<Option<ResolvedProfile>, StoreError> {
    let stored = store.get(name).await?;
    Ok(resolve_profile(name, stored))
}

/// Whether `name` names a resolvable profile — [`resolve`], so the existence rule has
/// exactly one implementation.
pub async fn exists(store: &dyn SandboxProfileRepository, name: &str) -> Result<bool, StoreError> {
    Ok(resolve(store, name).await?.is_some())
}

/// Every profile name the instance can serve, each with whether it is virtual.
pub async fn names(
    store: &dyn SandboxProfileRepository,
) -> Result<Vec<(String, bool)>, StoreError> {
    let rows = store.list().await?;
    Ok(profile_names(rows.into_iter().map(|d| d.name)))
}

/// The SHARED existence gate for any `sandbox` reference a client can store
/// (ADR-0031 §7): the instance default, a Trigger's `sandbox`. `""` is the
/// *clear* sentinel and `off` means "no sandbox", so both are always valid;
/// anything else must name a profile that resolves.
///
/// A **write-time** gate, deliberately not the authoritative one — the create-run
/// chokepoint is, and it re-resolves at every fire. This one exists so the error
/// lands where the user can fix it. The env tier (`PDO_DEFAULT_SANDBOX`) never
/// passes through here, which is why the settings page discloses a `reason`
/// beside the instance default.
pub async fn validate_ref(store: &dyn SandboxProfileRepository, raw: &str) -> Result<(), String> {
    let Some(mode) = SandboxMode::parse(raw) else {
        return Ok(()); // "" / whitespace = the clear sentinel
    };
    let Some(name) = mode.profile() else {
        return Ok(()); // `off`
    };
    match exists(store, name).await {
        Ok(true) => Ok(()),
        Ok(false) => Err(format!(
            "unknown sandbox profile `{name}`: use `off`, `{MINIMAL_PROFILE}`, \
             `{FULL_PROFILE}`, or a profile you created under Settings → Staging \
             profiles",
        )),
        Err(e) => Err(format!("sandbox profile store error: {e}")),
    }
}

#[cfg(test)]
pub(crate) mod fixture {
    use super::*;
    use crate::application::testing::{FakeHome, FixedReferents, InMemoryProfiles, Refusing};
    use pdo_kernel::extension_host::NoExtensions;

    pub(crate) struct Fixture {
        pub(crate) profiles: SandboxProfiles,
        pub(crate) store: Arc<InMemoryProfiles>,
    }

    pub(crate) fn with_home(present: &[&str]) -> Fixture {
        let store = Arc::new(InMemoryProfiles::default());
        Fixture {
            profiles: SandboxProfiles::new(
                store.clone(),
                Arc::new(NoExtensions),
                Arc::new(FakeHome::at("/home/u", present)),
                Arc::new(FixedReferents::default()),
            ),
            store,
        }
    }

    pub(crate) fn refusing() -> (Fixture, Arc<Refusing>) {
        let host = Arc::new(Refusing::default());
        let store = Arc::new(InMemoryProfiles::default());
        let fixture = Fixture {
            profiles: SandboxProfiles::new(
                store.clone(),
                host.clone(),
                Arc::new(FakeHome::at("/home/u", &[])),
                Arc::new(FixedReferents::default()),
            ),
            store,
        };
        (fixture, host)
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::with_home;
    use crate::domain::profile::ProfileDiff;

    #[tokio::test]
    async fn the_virtual_defaults_resolve_and_list_with_no_row() {
        let f = with_home(&[]);
        assert!(f.profiles.exists("full").await.unwrap());
        assert!(!f.profiles.exists("team").await.unwrap());
        assert_eq!(
            f.profiles.names().await.unwrap(),
            vec![("full".to_string(), true), ("minimal".to_string(), true)]
        );
    }

    #[tokio::test]
    async fn a_row_is_listed_and_resolved() {
        let f = with_home(&[]);
        f.store.seed(ProfileDiff {
            name: "team".into(),
            disabled: Vec::new(),
            extras: Vec::new(),
            env: Default::default(),
            image: None,
            updated_at: "t".into(),
        });
        let team = f.profiles.resolve("team").await.unwrap().unwrap();
        assert!(team.materialised && !team.is_virtual);
        assert_eq!(f.profiles.names().await.unwrap().len(), 3);
    }
}
