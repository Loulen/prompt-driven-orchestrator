//! What an Agent de support reads besides its prompt: the skills PDO manages,
//! delivered into its folder at each opening, and the core's code at the
//! instance's tag, cloned at the first demand, then reused, never for a
//! version without a public tag.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use pdo_harness::domain::agent_choice::AgentChoice;
use pdo_kernel::{Actor, ResourceKind, Verb};
use pdo_session::application::ports::BoxFuture;
use pdo_session::application::support::{
    ChoiceRefusal, CoreRepository, ResolvedAgent, Support, SupportAgents, SupportError,
    SupportLaunch, SupportSkills, SupportSources, SOURCE,
};
use pdo_session::domain::names::support_session_name_of;
use pdo_session::domain::support_sources::CoreSourceRefusal;

use crate::fakes::{FakeTmux, Host};

/// Every choice runs claude.
struct Claude;

impl SupportAgents for Claude {
    fn resolve<'a>(
        &'a self,
        _choice: &'a AgentChoice,
    ) -> BoxFuture<'a, Result<ResolvedAgent, ChoiceRefusal>> {
        Box::pin(async move {
            Ok(ResolvedAgent {
                harness: pdo_harness::registry::claude(),
                model: None,
                effort: None,
                label: "Custom".into(),
            })
        })
    }
}

/// Writes one `SKILL.md` per name into the folder it is given, and remembers
/// the folders.
#[derive(Default)]
struct Skills {
    names: Vec<String>,
    folders: Mutex<Vec<PathBuf>>,
    fail: bool,
}

impl SupportSkills for Skills {
    fn deliver<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<String>, String>> {
        Box::pin(async move {
            if self.fail {
                return Err("disk full".into());
            }
            self.folders.lock().unwrap().push(dir.to_path_buf());
            for name in &self.names {
                let skill = dir.join(".agents/skills").join(name);
                std::fs::create_dir_all(&skill).unwrap();
                std::fs::write(skill.join("SKILL.md"), name).unwrap();
            }
            Ok(self.names.clone())
        })
    }
}

/// A repository that has the tags it was given (or cannot be asked), and
/// clones by writing `.git` and a `README.md` in the folder; counts both.
#[derive(Default)]
struct Repository {
    tags: Vec<String>,
    unreachable: bool,
    asked: AtomicUsize,
    clones: AtomicUsize,
}

impl CoreRepository for Repository {
    fn has_tag<'a>(&'a self, tag: &'a str) -> BoxFuture<'a, Result<bool, String>> {
        Box::pin(async move {
            self.asked.fetch_add(1, Ordering::SeqCst);
            if self.unreachable {
                return Err("offline".into());
            }
            Ok(self.tags.iter().any(|t| t == tag))
        })
    }

    fn clone_at<'a>(&'a self, tag: &'a str, dest: &'a Path) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.clones.fetch_add(1, Ordering::SeqCst);
            std::fs::create_dir_all(dest.join(".git")).unwrap();
            std::fs::write(dest.join("README.md"), tag).unwrap();
            Ok(())
        })
    }
}

fn custom() -> AgentChoice {
    AgentChoice::Custom {
        harness: "claude".into(),
        model: None,
        effort: None,
    }
}

fn alice() -> Actor {
    Actor::Identified {
        namespace: "test".into(),
        id: "alice".into(),
    }
}

struct Fixture {
    root: tempfile::TempDir,
    tmux: Arc<FakeTmux>,
    host: Arc<Host>,
    skills: Arc<Skills>,
    repository: Arc<Repository>,
    support: Support,
}

impl Fixture {
    fn new(host: Arc<Host>, skills: Skills, repository: Repository) -> Self {
        let root = tempfile::tempdir().unwrap();
        let tmux = Arc::new(FakeTmux::default());
        let skills = Arc::new(skills);
        let repository = Arc::new(repository);
        let support = Support::new(
            tmux.clone(),
            Arc::new(Claude),
            host.clone(),
            SupportLaunch {
                root: root.path().join("support"),
                daemon_port: 6172,
                tmux_cmd_override: None,
            },
        )
        .with_sources(Arc::new(SupportSources::new(
            skills.clone(),
            repository.clone(),
            root.path().join("core-source"),
            "2.80.4",
        )));
        Self {
            root,
            tmux,
            host,
            skills,
            repository,
            support,
        }
    }

    fn folder(&self) -> PathBuf {
        self.root
            .path()
            .join("support")
            .join(support_session_name_of(&alice()))
    }

    fn instructions(&self) -> String {
        std::fs::read_to_string(self.folder().join("AGENTS.md")).unwrap()
    }

    fn cache(&self) -> PathBuf {
        self.root.path().join("core-source/v2.80.4")
    }
}

fn published() -> Repository {
    Repository {
        tags: vec!["v2.80.4".into()],
        ..Repository::default()
    }
}

fn skills(names: &[&str]) -> Skills {
    Skills {
        names: names.iter().map(|n| n.to_string()).collect(),
        ..Skills::default()
    }
}

#[tokio::test]
async fn the_folder_receives_the_skills_and_the_instructions_name_them_and_the_core_code() {
    let f = Fixture::new(
        Host::allowing(),
        skills(&["pdo-orchestrate", "test-cli"]),
        published(),
    );
    f.support
        .open(&alice(), &custom(), None, false)
        .await
        .unwrap();

    assert_eq!(*f.skills.folders.lock().unwrap(), vec![f.folder()]);
    assert!(f
        .folder()
        .join(".agents/skills/test-cli/SKILL.md")
        .is_file());
    let instructions = f.instructions();
    assert!(
        instructions.contains("pdo help"),
        "the prompt: {instructions}"
    );
    assert!(
        instructions.contains("`pdo-orchestrate`, `test-cli`"),
        "{instructions}"
    );
    assert!(instructions.contains("tag `v2.80.4`"), "{instructions}");
    assert!(
        instructions.contains("`pdo support source`"),
        "{instructions}"
    );
    assert_eq!(
        f.repository.clones.load(Ordering::SeqCst),
        0,
        "opening clones nothing"
    );

    // Reopened: the tag is not asked again.
    f.support
        .open(&alice(), &custom(), None, true)
        .await
        .unwrap();
    assert_eq!(f.repository.asked.load(Ordering::SeqCst), 1);
    assert_eq!(f.skills.folders.lock().unwrap().len(), 2, "delivered again");
}

#[tokio::test]
async fn the_core_code_is_cloned_at_the_first_demand_then_reused() {
    let f = Fixture::new(Host::allowing(), Skills::default(), published());
    let checkout = f.support.core_source(&alice()).await.unwrap();
    assert_eq!(checkout.tag, "v2.80.4");
    assert_eq!(checkout.dir, f.cache());
    assert_eq!(
        std::fs::read_to_string(f.cache().join("README.md")).unwrap(),
        "v2.80.4"
    );
    let again = f.support.core_source(&alice()).await.unwrap();
    assert_eq!(again, checkout);
    assert_eq!(f.repository.clones.load(Ordering::SeqCst), 1, "reused");

    // The Operation `session` × `source` on the agent's session.
    let asked = f.host.asked();
    assert_eq!(asked[0].resource, ResourceKind::Session);
    assert_eq!(asked[0].verb, Verb::Action(SOURCE.into()));
    assert_eq!(
        asked[0].target.as_deref(),
        Some(support_session_name_of(&alice()).as_str())
    );

    // An agent opened now is told where the clone is.
    f.support
        .open(&alice(), &custom(), None, false)
        .await
        .unwrap();
    let instructions = f.instructions();
    assert!(
        instructions.contains(&format!("cloned in `{}`", f.cache().display())),
        "{instructions}"
    );
}

#[tokio::test]
async fn a_version_without_a_public_tag_is_never_cloned_and_the_instructions_say_so() {
    let f = Fixture::new(Host::allowing(), Skills::default(), Repository::default());
    f.support
        .open(&alice(), &custom(), None, false)
        .await
        .unwrap();
    let instructions = f.instructions();
    assert!(
        instructions.contains("no public tag (`v2.80.4`)"),
        "{instructions}"
    );
    assert!(
        !instructions.contains("pdo support source"),
        "{instructions}"
    );

    let refused = f.support.core_source(&alice()).await;
    assert_eq!(
        refused,
        Err(SupportError::CoreSource(CoreSourceRefusal::NoPublicTag {
            tag: "v2.80.4".into()
        }))
    );
    assert_eq!(f.repository.clones.load(Ordering::SeqCst), 0, "no attempt");
    assert!(!f.cache().exists());
}

#[tokio::test]
async fn a_repository_that_cannot_be_asked_is_asked_again_on_the_demand_for_the_code() {
    let f = Fixture::new(
        Host::allowing(),
        Skills::default(),
        Repository {
            unreachable: true,
            ..Repository::default()
        },
    );
    f.support
        .open(&alice(), &custom(), None, false)
        .await
        .unwrap();
    assert!(
        f.instructions().contains("could not be checked (offline)"),
        "{}",
        f.instructions()
    );
    // The next opening does not wait for the repository again...
    f.support
        .open(&alice(), &custom(), None, true)
        .await
        .unwrap();
    assert_eq!(f.repository.asked.load(Ordering::SeqCst), 1);
    // ... the demand for the code asks again.
    let refused = f.support.core_source(&alice()).await;
    assert!(
        matches!(
            refused,
            Err(SupportError::CoreSource(
                CoreSourceRefusal::Unchecked { .. }
            ))
        ),
        "{refused:?}"
    );
    assert_eq!(f.repository.asked.load(Ordering::SeqCst), 2);
    assert_eq!(f.repository.clones.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn skills_that_cannot_be_written_refuse_the_opening() {
    let f = Fixture::new(
        Host::allowing(),
        Skills {
            fail: true,
            ..Skills::default()
        },
        published(),
    );
    let refused = f.support.open(&alice(), &custom(), None, false).await;
    assert!(
        matches!(refused, Err(SupportError::Folder(ref why)) if why.contains("disk full")),
        "{refused:?}"
    );
    assert!(f.tmux.created.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_refusing_extension_gives_no_code() {
    let f = Fixture::new(Host::refusing(), Skills::default(), published());
    assert!(matches!(
        f.support.core_source(&alice()).await,
        Err(SupportError::Refused(_))
    ));
    assert_eq!(f.repository.asked.load(Ordering::SeqCst), 0);
}
