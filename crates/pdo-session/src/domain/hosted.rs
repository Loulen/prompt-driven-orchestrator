//! A Session hébergée as the core records it (ADR-0092): what the extensions
//! read of it ([`HostedSession`], its Zoom du terminal included), plus what
//! only the core keeps (the conversation to resume), and the transitions of
//! its state. Pure: the clock is a parameter.

use std::path::{Path, PathBuf};

use pdo_extension_api::{
    Actor, EndReason, HostedConfig, HostedFolder, HostedSession, HostedState, OpenSession,
    MAX_TERMINAL_ZOOM, MIN_TERMINAL_ZOOM,
};
use serde::{Deserialize, Serialize};

/// One Session hébergée, as it is stored and survives a restart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedRecord {
    #[serde(flatten)]
    pub session: HostedSession,
    /// The conversation pinned at launch, for a harness that pins one.
    #[serde(default)]
    pub conversation_id: Option<String>,
    /// The harness that conversation runs on: a relaunch on another one
    /// starts a new conversation.
    #[serde(default)]
    pub harness: Option<String>,
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
            harness: None,
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

    /// Its process came up: `starting` becomes `running`. Whether it changed:
    /// a session terminated while it was being launched stays ended.
    pub fn run(&mut self, now: &str) -> bool {
        if self.session.state != HostedState::Starting {
            return false;
        }
        self.set(HostedState::Running, None, now);
        true
    }

    /// It ended, for `reason`. Whether it changed: an ended session keeps
    /// the reason it ended for.
    pub fn end(&mut self, reason: EndReason, now: &str) -> bool {
        if !self.is_live() {
            return false;
        }
        self.set(HostedState::Ended, Some(reason), now);
        true
    }

    /// `actor` relaunches it: an ended session is `starting` again, started
    /// by `actor`. Whether it changed: only an ended session relaunches.
    pub fn relaunch(&mut self, actor: &Actor, now: &str) -> bool {
        if self.is_live() {
            return false;
        }
        self.session.started_by = Some(actor.clone());
        self.set(HostedState::Starting, None, now);
        true
    }

    /// Its agent declared it waits for a person, saying `message`: a running
    /// session is `awaiting_user`. `None` when nothing changed (not running,
    /// or already waiting on the same message).
    pub fn await_user(&mut self, message: Option<String>, now: &str) -> Option<WaitDeclared> {
        let declared = match self.session.state {
            HostedState::Running => WaitDeclared::Declared,
            HostedState::AwaitingUser if self.session.wait_message != message => {
                WaitDeclared::Refreshed
            }
            _ => return None,
        };
        self.set(HostedState::AwaitingUser, None, now);
        self.session.wait_message = message;
        Some(declared)
    }

    /// A person answered (Enter on its terminal): an `awaiting_user` session
    /// runs again. Whether it changed.
    pub fn resume(&mut self, now: &str) -> bool {
        if self.session.state != HostedState::AwaitingUser {
            return false;
        }
        self.set(HostedState::Running, None, now);
        true
    }

    /// Its configuration is now `config`, for its next launch. Whether it
    /// changed.
    pub fn configure(&mut self, config: HostedConfig, now: &str) -> bool {
        if self.session.config == config {
            return false;
        }
        self.session.config = config;
        self.session.updated_at = now.to_string();
        true
    }

    /// Its Zoom du terminal is now `zoom` percent, for every Poste. Whether it
    /// changed; a zoom out of range is refused and changes nothing.
    pub fn zoom(&mut self, zoom: u16, now: &str) -> Result<bool, ZoomOutOfRange> {
        let zoom = terminal_zoom(zoom)?;
        if self.session.terminal_zoom == zoom {
            return Ok(false);
        }
        self.session.terminal_zoom = zoom;
        self.session.updated_at = now.to_string();
        Ok(true)
    }

    fn set(&mut self, state: HostedState, reason: Option<EndReason>, now: &str) {
        self.session.state = state;
        self.session.end_reason = reason;
        self.session.wait_message = None;
        self.session.updated_at = now.to_string();
    }
}

/// A Zoom du terminal outside [`MIN_TERMINAL_ZOOM`]..=[`MAX_TERMINAL_ZOOM`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoomOutOfRange(pub u16);

impl std::fmt::Display for ZoomOutOfRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "a terminal zoom is {MIN_TERMINAL_ZOOM} to {MAX_TERMINAL_ZOOM} %, not {}",
            self.0
        )
    }
}

/// `zoom` as a Zoom du terminal, or why it is none.
pub fn terminal_zoom(zoom: u16) -> Result<u16, ZoomOutOfRange> {
    if (MIN_TERMINAL_ZOOM..=MAX_TERMINAL_ZOOM).contains(&zoom) {
        Ok(zoom)
    } else {
        Err(ZoomOutOfRange(zoom))
    }
}

/// What a declared wait did to a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitDeclared {
    /// It was running: it now waits.
    Declared,
    /// It waited already: its message changed.
    Refreshed,
}

/// How an agent session relaunches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Relaunch {
    /// Back into the conversation pinned at its last launch, by its identity.
    Resume { conversation_id: String },
    /// A new conversation, pinned to this identity when the harness pins
    /// one: the extensions' text and the initial prompt are read again.
    Fresh { conversation_id: Option<String> },
}

/// What a relaunch knows of the harness it relaunches on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelaunchHarness<'a> {
    pub name: &'a str,
    /// It re-enters a conversation by its identity.
    pub resumes_by_id: bool,
    /// It pins a conversation's identity at launch.
    pins: bool,
}

impl<'a> RelaunchHarness<'a> {
    pub fn new(name: &'a str, resumes_by_id: bool, pins: bool) -> Self {
        Self {
            name,
            resumes_by_id,
            pins,
        }
    }
}

/// How the agent session `record` relaunches on `harness`: it resumes its
/// conversation when one was pinned on that same harness (a record that does
/// not name its harness was launched on it), the harness re-enters a
/// conversation by its identity, and that conversation was written
/// (`written`: `Some(false)` when the core saw it was not, an agent nobody
/// spoke to; `None` when it cannot tell). Otherwise a new conversation
/// starts, pinned to `new_id` when the harness pins one. Never a blind
/// continue: in a folder shared with others, it could pick someone else's
/// conversation.
pub fn relaunch_of(
    record: &HostedRecord,
    harness: RelaunchHarness<'_>,
    written: Option<bool>,
    new_id: impl FnOnce() -> String,
) -> Relaunch {
    let same_harness = record.harness.as_deref().is_none_or(|h| h == harness.name);
    match &record.conversation_id {
        Some(id) if harness.resumes_by_id && same_harness && written != Some(false) => {
            Relaunch::Resume {
                conversation_id: id.clone(),
            }
        }
        _ => Relaunch::Fresh {
            conversation_id: harness.pins.then(new_id),
        },
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

/// The `PDO_LIVE_DIR` of a session, in its state folder: its Artefacts live,
/// kept from one relaunch to the next.
pub fn live_dir(state_dir: &Path) -> PathBuf {
    state_dir.join("live")
}

/// The file the Artefact live `path` names under `live_dir`; `None` for a
/// path that would leave it (absolute, `..`, empty).
pub fn live_file(live_dir: &Path, path: &str) -> Option<PathBuf> {
    let mut file = live_dir.to_path_buf();
    let mut parts = 0;
    for part in path.split('/') {
        match part {
            "" | "." => continue,
            ".." => return None,
            part if part.contains('\\') => return None,
            part => {
                file.push(part);
                parts += 1;
            }
        }
    }
    (parts > 0 && !path.starts_with('/')).then_some(file)
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
        assert!(record.run("t1"));
        assert_eq!(record.session.state, HostedState::Running);
        assert_eq!(record.session.updated_at, "t1");
        assert!(!record.run("t1b"), "only a starting session comes up");
        assert!(record.end(EndReason::Exited, "t2"));
        assert!(!record.is_live());
        assert_eq!(record.session.end_reason, Some(EndReason::Exited));
        assert_eq!(record.session.created_at, "t0");
        assert!(!record.end(EndReason::Terminated, "t3"), "ended once");
        assert!(!record.run("t4"), "an ended session never runs again");
        assert_eq!(record.session.end_reason, Some(EndReason::Exited));
        assert_eq!(record.session.updated_at, "t2");
    }

    fn alice() -> Actor {
        Actor::Identified {
            namespace: "test".into(),
            id: "alice".into(),
        }
    }

    #[test]
    fn an_ended_record_relaunches_starting_with_who_relaunched_it() {
        let mut record = HostedRecord::opening(
            "pdo-hosted-1",
            "test",
            &OpenSession::agent("k"),
            Path::new("/w"),
            "t0",
        );
        assert!(!record.relaunch(&alice(), "t1"), "a live session does not");
        record.run("t1");
        record.end(EndReason::Terminated, "t2");
        assert!(record.relaunch(&alice(), "t3"));
        assert_eq!(record.session.state, HostedState::Starting);
        assert_eq!(record.session.end_reason, None);
        assert_eq!(record.session.started_by, Some(alice()));
        assert_eq!(record.session.updated_at, "t3");
        assert!(record.run("t4"), "and comes up again");
    }

    #[test]
    fn a_running_record_waits_then_resumes_and_a_repeat_changes_nothing() {
        let mut record = HostedRecord::opening(
            "pdo-hosted-1",
            "test",
            &OpenSession::agent("k"),
            Path::new("/w"),
            "t0",
        );
        assert_eq!(record.await_user(None, "t1"), None, "not running yet");
        record.run("t1");
        assert_eq!(
            record.await_user(Some("Which DB?".into()), "t2"),
            Some(WaitDeclared::Declared)
        );
        assert_eq!(record.session.state, HostedState::AwaitingUser);
        assert_eq!(record.session.wait_message.as_deref(), Some("Which DB?"));
        assert_eq!(record.await_user(Some("Which DB?".into()), "t3"), None);
        assert_eq!(record.await_user(None, "t3"), Some(WaitDeclared::Refreshed));
        assert!(record.resume("t4"));
        assert_eq!(record.session.state, HostedState::Running);
        assert!(!record.resume("t5"), "it runs already");
        record.await_user(Some("again".into()), "t6");
        record.end(EndReason::Exited, "t7");
        assert_eq!(record.session.wait_message, None, "an end clears the wait");
    }

    #[test]
    fn a_configuration_changes_only_when_it_differs() {
        let mut record = HostedRecord::opening(
            "pdo-hosted-1",
            "test",
            &OpenSession::agent("k"),
            Path::new("/w"),
            "t0",
        );
        assert!(!record.configure(HostedConfig::default(), "t1"));
        assert!(record.configure(HostedConfig::default().with_model("opus"), "t2"));
        assert_eq!(record.session.config.model.as_deref(), Some("opus"));
        assert_eq!(record.session.updated_at, "t2");
        assert_eq!(
            record.session.state,
            HostedState::Starting,
            "its state is not touched"
        );
    }

    #[test]
    fn a_relaunch_resumes_the_conversation_of_the_same_harness_by_its_identity_only() {
        let mut record = HostedRecord::opening(
            "pdo-hosted-1",
            "test",
            &OpenSession::agent("k"),
            Path::new("/w"),
            "t0",
        );
        record.conversation_id = Some("c1".into());
        record.harness = Some("claude".into());
        let new = || "c2".to_string();
        let claude = RelaunchHarness::new("claude", true, true);
        let resumed = Relaunch::Resume {
            conversation_id: "c1".into(),
        };
        let fresh = Relaunch::Fresh {
            conversation_id: Some("c2".into()),
        };
        assert_eq!(relaunch_of(&record, claude, Some(true), new), resumed);
        assert_eq!(
            relaunch_of(&record, claude, None, new),
            resumed,
            "the core cannot tell: it tries"
        );
        assert_eq!(
            relaunch_of(&record, claude, Some(false), new),
            fresh,
            "a conversation never written: a new one"
        );
        assert_eq!(
            relaunch_of(
                &record,
                RelaunchHarness::new("other", true, true),
                None,
                new
            ),
            fresh,
            "another harness: a new conversation"
        );
        assert_eq!(
            relaunch_of(
                &record,
                RelaunchHarness::new("claude", false, false),
                None,
                new
            ),
            Relaunch::Fresh {
                conversation_id: None
            },
            "a harness that cannot re-enter by identity"
        );
        record.harness = None;
        assert_eq!(
            relaunch_of(&record, claude, None, new),
            resumed,
            "a record that does not name its harness was launched on it"
        );
        record.conversation_id = None;
        assert_eq!(
            relaunch_of(&record, claude, None, new),
            fresh,
            "nothing was pinned"
        );
    }

    #[test]
    fn an_artefact_live_never_leaves_its_folder() {
        let live = live_dir(Path::new("/s"));
        assert_eq!(live, PathBuf::from("/s/live"));
        assert_eq!(
            live_file(&live, "notes/plan.md"),
            Some(PathBuf::from("/s/live/notes/plan.md"))
        );
        assert_eq!(
            live_file(&live, "./a.md"),
            Some(PathBuf::from("/s/live/a.md"))
        );
        for bad in ["../x", "a/../../x", "/etc/passwd", "", "/", "a\\b"] {
            assert_eq!(live_file(&live, bad), None, "{bad}");
        }
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
    fn a_zoom_stays_within_its_range_and_says_whether_it_changed() {
        let open = OpenSession::shell("s");
        let mut record =
            HostedRecord::opening("pdo-hosted-3", "test", &open, Path::new("/h"), "t0");
        assert_eq!(record.zoom(150, "t1"), Ok(true));
        assert_eq!(record.session.terminal_zoom, 150);
        assert_eq!(record.session.updated_at, "t1");
        assert_eq!(record.zoom(150, "t2"), Ok(false));
        assert_eq!(record.session.updated_at, "t1");
        assert_eq!(record.zoom(49, "t3"), Err(ZoomOutOfRange(49)));
        assert_eq!(record.zoom(201, "t3"), Err(ZoomOutOfRange(201)));
        assert_eq!(record.zoom(50, "t4"), Ok(true));
        assert_eq!(record.zoom(200, "t5"), Ok(true));
        assert_eq!(record.session.terminal_zoom, 200);
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
