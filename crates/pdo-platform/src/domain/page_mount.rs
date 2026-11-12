//! Page mounts: a directory served live under `/pages/<name>/`, with an
//! optional reading guide (`page_guide`) PDO shows beside it.
//!
//! The rules here are pure: what a mount name may be, which directories may
//! never be mounted, how a requested path is split into the components the
//! file reader resolves one by one beneath the mount (never a `..`, never an
//! absolute path), and who owns a mount.

use std::ffi::{OsStr, OsString};
use std::path::{Component, Path};

use pdo_kernel::Actor;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::page_guide::GuideEntry;

/// A directory bound to a name. `run_id` is the Run whose agent mounted it, if
/// any: archiving that Run removes the mount, and its guide with it.
///
/// It has an owner and a Projet: reading it (the list, the page) is judged
/// `page_mount` × `read` in its Projet, unmounting it is its owner's, or
/// `page_mount` × `delete` in its Projet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PageMount {
    pub name: String,
    pub directory: String,
    pub run_id: Option<String>,
    /// The node whose agent mounted it, beside `run_id`.
    #[serde(default)]
    pub node_id: Option<String>,
    pub created_at: String,
    /// The reading guide given at mount time; `None` for a page served alone.
    #[serde(default)]
    pub guide: Option<Vec<GuideEntry>>,
    /// Who mounted it: the Actor, or for an agent the one who created its
    /// Run. `None` when nobody is known (a mount older than its owner, whose
    /// Run names no creator).
    #[serde(default)]
    pub owner: Option<Actor>,
    /// The Projet it is in: its Run's for an agent's mount, the one asked for
    /// otherwise; `None` outside any Projet.
    #[serde(default)]
    pub project_id: Option<String>,
}

impl PageMount {
    /// The Projets its Operations are judged in: its own, or none.
    pub fn projects(&self) -> Vec<String> {
        self.project_id.iter().cloned().collect()
    }

    /// Whether `actor` owns the mount. An agent never does: its claim to
    /// speak for a Run is judged by the extensions, not taken for its owner.
    pub fn is_owned_by(&self, actor: &Actor) -> bool {
        self.owner.as_ref() == Some(actor)
    }
}

/// Why a mount was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MountError {
    #[error("{0}")]
    Invalid(String),
    #[error("page mount `{0}` is already mounted")]
    Duplicate(String),
}

/// A mount name matches `[a-z0-9_-]{1,64}`: it is a URL segment.
pub fn validate_name(name: &str) -> Result<(), MountError> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-".contains(&byte))
    {
        return Err(MountError::Invalid(
            "page mount name must match [a-z0-9_-]{1,64}".to_string(),
        ));
    }
    Ok(())
}

/// A canonical directory may be mounted unless it sits under a `.pdo/artifacts`
/// tree: node artifacts are served by the Run, never as a page.
pub fn check_mountable(canonical: &Path) -> Result<(), MountError> {
    let components: Vec<&OsStr> = canonical.components().map(|c| c.as_os_str()).collect();
    if components
        .windows(2)
        .any(|pair| pair[0] == ".pdo" && pair[1] == "artifacts")
    {
        return Err(MountError::Invalid(
            "directories under .pdo/artifacts cannot be mounted".to_string(),
        ));
    }
    Ok(())
}

/// The components of a requested path, relative to the mount root. `None` when
/// the path tries to leave the mount (`..`, an absolute path, a prefix). An
/// empty list is the mount root itself.
pub fn requested_components(requested: &str) -> Option<Vec<OsString>> {
    let mut components = Vec::new();
    for component in Path::new(requested).components() {
        match component {
            Component::Normal(name) => components.push(name.to_owned()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(components)
}

/// The file a directory request serves.
pub const INDEX_FILE: &str = "index.html";

#[cfg(test)]
mod tests {
    use super::*;

    fn mount(owner: Option<Actor>, run_id: Option<&str>) -> PageMount {
        PageMount {
            name: "demo".into(),
            directory: "/srv/demo".into(),
            run_id: run_id.map(str::to_string),
            node_id: None,
            created_at: "now".into(),
            guide: None,
            owner,
            project_id: Some("p1".into()),
        }
    }

    fn named(id: &str) -> Actor {
        Actor::Identified {
            namespace: "test".into(),
            id: id.into(),
        }
    }

    #[test]
    fn a_mount_is_owned_by_its_owner_only() {
        let alices = mount(Some(named("alice")), Some("r1"));
        assert!(alices.is_owned_by(&named("alice")));
        assert!(!alices.is_owned_by(&named("bob")));
        assert!(!alices.is_owned_by(&Actor::LocalOperator));
        let agent = |run_id: &str| Actor::Agent {
            run_id: run_id.into(),
            node_id: "n1".into(),
        };
        assert!(!alices.is_owned_by(&agent("r1")), "a claim, judged");
        let nobodys = mount(None, None);
        assert!(!nobodys.is_owned_by(&named("alice")));
        assert!(!nobodys.is_owned_by(&agent("r1")));
        assert_eq!(alices.projects(), ["p1"]);
    }

    #[test]
    fn names_are_url_segments() {
        assert!(validate_name("demo-1_a").is_ok());
        assert!(validate_name("").is_err());
        assert!(validate_name("Demo").is_err());
        assert!(validate_name("a/b").is_err());
        assert!(validate_name(&"a".repeat(65)).is_err());
        assert!(validate_name(&"a".repeat(64)).is_ok());
    }

    #[test]
    fn artifacts_are_never_mountable() {
        assert!(check_mountable(Path::new("/repo/.pdo/artifacts/n1")).is_err());
        assert!(check_mountable(Path::new("/repo/.pdo/pages")).is_ok());
        assert!(check_mountable(Path::new("/repo/artifacts")).is_ok());
    }

    #[test]
    fn requested_paths_stay_inside_the_mount() {
        assert_eq!(requested_components(""), Some(vec![]));
        assert_eq!(
            requested_components("./a/b.html"),
            Some(vec![OsString::from("a"), OsString::from("b.html")])
        );
        assert_eq!(requested_components("../../etc/passwd"), None);
        assert_eq!(requested_components("/etc/passwd"), None);
        assert_eq!(requested_components("a/../b"), None);
    }
}
