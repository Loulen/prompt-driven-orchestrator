//! A Session hébergée as the core records it (ADR-0091): what the extensions
//! read of it ([`HostedSession`]), plus what only the core keeps (the
//! conversation to resume, the Zoom du terminal), and the transitions of its
//! state. Pure: the clock is a parameter.

use std::path::{Path, PathBuf};

use pdo_extension_api::{EndReason, HostedFolder, HostedSession, HostedState, OpenSession};
use serde::{Deserialize, Serialize};

/// The Zoom du terminal of a session nobody changed, in percent.
pub const DEFAULT_TERMINAL_ZOOM: u16 = 100;

fn default_zoom() -> u16 {
    DEFAULT_TERMINAL_ZOOM
}

/// One Session hébergée, as it is stored and survives a restart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedRecord {
    #[serde(flatten)]
    pub session: HostedSession,
    /// The conversation pinned at launch, for a harness that pins one.
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default = "default_zoom")]
    pub terminal_zoom: u16,
}

impl HostedRecord {
    /// The record of the session `id` the extension `extension` asks for
    /// with `open`, working in `cwd`: starting, opened `now`.
    pub fn opening(id: &str, extension: &str, open: &OpenSession, cwd: &Path, now: &str) -> Self {
        let mut session =
            HostedSession::new(id, extension, &open.key, open.kind, cwd.to_string_lossy());
        session.config = open.config.clone();
        session.folder = open.folder.clone();
        session.project = open.project.clone();
        session.created_at = now.to_string();
        session.updated_at = now.to_string();
        Self {
            session,
            conversation_id: None,
            terminal_zoom: DEFAULT_TERMINAL_ZOOM,
        }
    }

    /// The session's name (its tmux session).
    pub fn id(&self) -> &str {
        &self.session.id
    }

    /// Whether it holds a process, hence a slot of the Cap.
    pub fn is_live(&self) -> bool {
        self.session.state.is_live()
    }

    /// The Projets its Operations are in.
    pub fn projects(&self) -> Vec<String> {
        self.session.project.iter().cloned().collect()
    }

    /// Its process came up.
    pub fn run(&mut self, now: &str) {
        self.set(HostedState::Running, None, now);
    }

    /// It ended, for `reason`.
    pub fn end(&mut self, reason: EndReason, now: &str) {
        self.set(HostedState::Ended, Some(reason), now);
    }

    fn set(&mut self, state: HostedState, reason: Option<EndReason>, now: &str) {
        self.session.state = state;
        self.session.end_reason = reason;
        self.session.updated_at = now.to_string();
    }
}

/// Why the folder of a Session hébergée does not resolve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderError {
    /// A path that is not absolute.
    Relative(String),
    /// The primary repository of a Projet was asked for, with no Projet.
    NoProject,
}

/// What the folder `folder` needs resolved: a path as it is, the primary
/// repository of the Projet `project`, or the home folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderLookup {
    Path(PathBuf),
    ProjectRepo(String),
    Home,
}

/// Read `folder` (in the Projet `project`) as what is left to look up.
pub fn folder_lookup(
    folder: &HostedFolder,
    project: Option<&str>,
) -> Result<FolderLookup, FolderError> {
    match folder {
        HostedFolder::Path { path } => {
            let path = PathBuf::from(path);
            if path.is_absolute() {
                Ok(FolderLookup::Path(path))
            } else {
                Err(FolderError::Relative(path.to_string_lossy().into_owned()))
            }
        }
        HostedFolder::ProjectRepo => project
            .map(|p| FolderLookup::ProjectRepo(p.to_string()))
            .ok_or(FolderError::NoProject),
        _ => Ok(FolderLookup::Home),
    }
}

/// The file the agent of a session reads first, in its state folder.
pub fn primer_path(state_dir: &Path) -> PathBuf {
    state_dir.join("primer.md")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdo_extension_api::{HostedConfig, HostedKind};

    #[test]
    fn a_record_opens_starting_then_runs_then_ends_with_its_reason() {
        let open = OpenSession::agent("k")
            .in_project("p1")
            .with_config(HostedConfig::default().with_model("opus"));
        let mut record =
            HostedRecord::opening("pdo-hosted-1", "test", &open, Path::new("/w"), "t0");
        assert_eq!(record.session.state, HostedState::Starting);
        assert_eq!(record.session.kind, HostedKind::Agent);
        assert_eq!(record.session.cwd, "/w");
        assert_eq!(record.session.config.model.as_deref(), Some("opus"));
        assert_eq!(record.projects(), ["p1"]);
        assert!(record.is_live());
        record.run("t1");
        assert_eq!(record.session.state, HostedState::Running);
        assert_eq!(record.session.updated_at, "t1");
        record.end(EndReason::Exited, "t2");
        assert!(!record.is_live());
        assert_eq!(record.session.end_reason, Some(EndReason::Exited));
        assert_eq!(record.session.created_at, "t0");
    }

    #[test]
    fn a_record_reads_back_from_its_json_with_its_defaults() {
        let open = OpenSession::shell("s");
        let record = HostedRecord::opening("pdo-hosted-2", "test", &open, Path::new("/h"), "t0");
        let json = serde_json::to_value(&record).unwrap();
        assert_eq!(json["id"], "pdo-hosted-2");
        assert_eq!(json["terminal_zoom"], 100);
        let mut legacy = json.clone();
        legacy.as_object_mut().unwrap().remove("terminal_zoom");
        let back: HostedRecord = serde_json::from_value(legacy).unwrap();
        assert_eq!(back, record);
    }

    #[test]
    fn a_folder_resolves_to_a_path_a_projet_or_the_home() {
        let path = HostedFolder::Path {
            path: "/srv/x".into(),
        };
        assert_eq!(
            folder_lookup(&path, None),
            Ok(FolderLookup::Path("/srv/x".into()))
        );
        let relative = HostedFolder::Path { path: "x".into() };
        assert_eq!(
            folder_lookup(&relative, None),
            Err(FolderError::Relative("x".into()))
        );
        assert_eq!(
            folder_lookup(&HostedFolder::ProjectRepo, Some("p1")),
            Ok(FolderLookup::ProjectRepo("p1".into()))
        );
        assert_eq!(
            folder_lookup(&HostedFolder::ProjectRepo, None),
            Err(FolderError::NoProject)
        );
        assert_eq!(
            folder_lookup(&HostedFolder::Home, Some("p1")),
            Ok(FolderLookup::Home)
        );
        assert_eq!(primer_path(Path::new("/s")), PathBuf::from("/s/primer.md"));
    }
}
