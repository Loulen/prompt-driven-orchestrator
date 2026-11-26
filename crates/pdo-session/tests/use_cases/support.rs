//! The Agent de support: open on a choice of agent, one per Actor, ask, read,
//! close; a choice that resolves to nothing runnable is refused with its
//! reason.

use std::path::Path;
use std::sync::Arc;

use pdo_harness::domain::agent_choice::AgentChoice;
use pdo_kernel::extension_host::NodeContribution;
use pdo_kernel::{Actor, ResourceKind, Verb};
use pdo_session::application::ports::BoxFuture;
use pdo_session::application::support::{
    ChoiceRefusal, ResolvedAgent, Support, SupportAgents, SupportError, SupportLaunch,
};
use pdo_session::domain::names::support_session_name_of;

use crate::fakes::{FakeTmux, Host};

/// Resolves the profile `deep` to `claude · opus · high`, a custom
/// combination on `claude` as given, and refuses the rest as a real
/// resolution would: `copilot` is not installed, any other name unknown.
pub(crate) struct Agents;

impl SupportAgents for Agents {
    fn resolve<'a>(
        &'a self,
        choice: &'a AgentChoice,
    ) -> BoxFuture<'a, Result<ResolvedAgent, ChoiceRefusal>> {
        Box::pin(async move {
            let (label, harness, model, effort) = match choice {
                AgentChoice::Inherit => return Err(ChoiceRefusal::Inherit),
                AgentChoice::Profile { profile_id } if profile_id == "deep" => (
                    "Deep".to_string(),
                    "claude".to_string(),
                    Some("opus".to_string()),
                    Some("high".to_string()),
                ),
                AgentChoice::Profile { profile_id } => {
                    return Err(ChoiceRefusal::UnknownProfile(profile_id.clone()))
                }
                AgentChoice::Custom {
                    harness,
                    model,
                    effort,
                } => (
                    "Custom".to_string(),
                    harness.clone(),
                    model.clone(),
                    effort.clone(),
                ),
            };
            match harness.as_str() {
                "claude" => Ok(ResolvedAgent {
                    harness: pdo_harness::registry::claude(),
                    model,
                    effort,
                    label,
                }),
                "copilot" => Err(ChoiceRefusal::NotInstalled("copilot".into())),
                other => Err(ChoiceRefusal::UnknownHarness(other.into())),
            }
        })
    }
}

pub(crate) fn support(
    root: &Path,
    tmux: Arc<FakeTmux>,
    host: Arc<Host>,
    seam: Option<&str>,
) -> Support {
    Support::new(
        tmux,
        Arc::new(Agents),
        host,
        SupportLaunch {
            root: root.to_path_buf(),
            daemon_port: 6172,
            tmux_cmd_override: seam.map(String::from),
        },
    )
}

pub(crate) fn deep() -> AgentChoice {
    AgentChoice::Profile {
        profile_id: "deep".into(),
    }
}

fn custom(harness: &str) -> AgentChoice {
    AgentChoice::Custom {
        harness: harness.into(),
        model: None,
        effort: None,
    }
}

pub(crate) fn alice() -> Actor {
    Actor::Identified {
        namespace: "test".into(),
        id: "alice".into(),
    }
}

#[tokio::test]
async fn the_agent_opens_in_its_folder_with_the_prompt_the_question_and_the_actor_token() {
    let root = tempfile::tempdir().unwrap();
    let tmux = Arc::new(FakeTmux::default());
    let host = Host::allowing();
    *host.contribution.lock().unwrap() = NodeContribution {
        env: [("PDO_TOKEN".to_string(), "token-of-alice".to_string())].into(),
        preamble: Some("EXTENSION TEXT".into()),
    };
    let uc = support(root.path(), tmux.clone(), host.clone(), None);

    let opened = uc
        .open(&alice(), &deep(), Some("which Runs failed?"), false)
        .await
        .unwrap();
    let session = support_session_name_of(&alice());
    assert_eq!(opened.session, session);
    assert_eq!(opened.agent.label, "Deep");
    assert_eq!(opened.agent.harness, "claude");
    assert_eq!(opened.agent.model.as_deref(), Some("opus"));

    let (name, cwd, script) = tmux.created.lock().unwrap()[0].clone();
    assert_eq!(name, session);
    assert_eq!(cwd, root.path().join(&session));
    assert!(
        script.contains("export PDO_TOKEN='token-of-alice'"),
        "{script}"
    );
    assert!(script.contains("--model 'opus'"), "{script}");
    assert!(script.contains("--effort 'high'"), "{script}");
    // Its instructions are in the files the harness loads; only the question
    // is its first message.
    let instructions = std::fs::read_to_string(cwd.join("AGENTS.md")).unwrap();
    assert!(
        instructions.starts_with("EXTENSION TEXT\n\n"),
        "{instructions}"
    );
    assert!(
        instructions.contains("pdo help"),
        "the embedded prompt: {instructions}"
    );
    assert!(
        !instructions.contains("which Runs failed?"),
        "{instructions}"
    );
    assert_eq!(
        std::fs::read_to_string(cwd.join("CLAUDE.md")).unwrap(),
        "@AGENTS.md\n"
    );
    let question = cwd.join("question.md");
    assert_eq!(
        std::fs::read_to_string(&question).unwrap(),
        "which Runs failed?\n"
    );
    assert!(
        script.ends_with(&format!("\"$(cat '{}')\"", question.display())),
        "{script}"
    );

    let launch = &host.launches.lock().unwrap()[0];
    assert_eq!(
        launch.session.kind,
        pdo_extension_api::SessionKind::SupportAgent
    );
    assert_eq!(launch.started_by, Some(alice()));
    assert_eq!(
        launch.working_dir.as_deref(),
        Some(cwd.to_str().unwrap()),
        "the extensions see the folder the agent runs in"
    );
    let asked = host.asked();
    assert_eq!(asked[0].resource, ResourceKind::Session);
    assert_eq!(asked[0].verb, Verb::Create);
    assert_eq!(asked[0].target.as_deref(), Some(session.as_str()));
}

#[tokio::test]
async fn without_a_question_the_agent_opens_on_an_empty_input() {
    let root = tempfile::tempdir().unwrap();
    let tmux = Arc::new(FakeTmux::default());
    let uc = support(root.path(), tmux.clone(), Host::allowing(), None);
    uc.open(&alice(), &deep(), Some("which Runs failed?"), false)
        .await
        .unwrap();
    let dir = root.path().join(support_session_name_of(&alice()));
    std::fs::write(dir.join("support-agent.md"), "an earlier version's primer").unwrap();
    uc.open(&alice(), &deep(), Some("  "), true).await.unwrap();

    let (_, cwd, script) = tmux.created.lock().unwrap()[1].clone();
    assert!(!script.contains("$(cat"), "no first message: {script}");
    assert!(
        !cwd.join("question.md").exists(),
        "the earlier question is not asked again"
    );
    assert!(cwd.join("AGENTS.md").exists());
    assert!(
        !cwd.join("support-agent.md").exists(),
        "the primer of an earlier version is gone"
    );
}

#[tokio::test]
async fn one_agent_per_actor_and_another_one_replaces_it_only_when_asked() {
    let root = tempfile::tempdir().unwrap();
    let tmux = Arc::new(FakeTmux::default());
    let uc = support(
        root.path(),
        tmux.clone(),
        Host::allowing(),
        Some("exec sleep 600"),
    );
    uc.open(&alice(), &deep(), None, false).await.unwrap();
    uc.open(&Actor::LocalOperator, &deep(), None, false)
        .await
        .unwrap();
    assert_eq!(
        tmux.created.lock().unwrap().len(),
        2,
        "two Actors, two agents"
    );

    let again = uc.open(&alice(), &custom("claude"), None, false).await;
    match again {
        Err(SupportError::AlreadyOpen(Some(agent))) => assert_eq!(agent.label, "Deep"),
        other => panic!("{other:?}"),
    }
    // A replacement refused closes nothing.
    assert_eq!(
        uc.open(&alice(), &custom("copilot"), None, true).await,
        Err(SupportError::Choice(ChoiceRefusal::NotInstalled(
            "copilot".into()
        )))
    );
    assert!(tmux.killed.lock().unwrap().is_empty());
    let replaced = uc
        .open(&alice(), &custom("claude"), None, true)
        .await
        .unwrap();
    assert_eq!(replaced.agent.label, "Custom");
    assert_eq!(
        tmux.killed.lock().unwrap().as_slice(),
        [support_session_name_of(&alice())]
    );
    assert_eq!(tmux.created.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn a_choice_that_cannot_run_here_is_refused_with_its_reason() {
    let root = tempfile::tempdir().unwrap();
    let tmux = Arc::new(FakeTmux::default());
    let uc = support(root.path(), tmux.clone(), Host::allowing(), None);
    for (choice, refusal) in [
        (
            custom("copilot"),
            ChoiceRefusal::NotInstalled("copilot".into()),
        ),
        (
            custom("ghost"),
            ChoiceRefusal::UnknownHarness("ghost".into()),
        ),
        (
            AgentChoice::Profile {
                profile_id: "gone".into(),
            },
            ChoiceRefusal::UnknownProfile("gone".into()),
        ),
        (AgentChoice::Inherit, ChoiceRefusal::Inherit),
    ] {
        assert_eq!(
            uc.open(&alice(), &choice, Some("hi"), false).await,
            Err(SupportError::Choice(refusal))
        );
    }
    assert!(tmux.created.lock().unwrap().is_empty());
    assert!(ChoiceRefusal::NotInstalled("copilot".into())
        .to_string()
        .contains("not installed"),);
}

#[tokio::test]
async fn a_question_goes_into_the_live_agent_as_the_person_types_it() {
    let root = tempfile::tempdir().unwrap();
    let tmux = Arc::new(FakeTmux::default());
    let host = Host::allowing();
    let uc = support(root.path(), tmux.clone(), host.clone(), None);
    assert_eq!(uc.ask(&alice(), "hi"), Err(SupportError::NotOpen));
    uc.open(&alice(), &deep(), None, false).await.unwrap();
    assert_eq!(uc.ask(&alice(), "  \n"), Err(SupportError::EmptyQuestion));

    let session = uc.ask(&alice(), " how many Pipelines? ").unwrap();
    assert_eq!(
        tmux.pasted_without_enter.lock().unwrap().as_slice(),
        [(session.clone(), "how many Pipelines?".to_string())]
    );
    assert_eq!(
        tmux.entered.lock().unwrap().as_slice(),
        std::slice::from_ref(&session)
    );
    // A paste tmux refuses is said, not taken for asked.
    *tmux.refuse_paste.lock().unwrap() = true;
    assert_eq!(uc.ask(&alice(), "again?"), Err(SupportError::NotDelivered));
    let ask = host
        .asked()
        .into_iter()
        .find(|o| o.verb == Verb::Action("ask".into()))
        .unwrap();
    assert_eq!(ask.target.as_deref(), Some(session.as_str()));
}

#[tokio::test]
async fn the_status_says_the_agent_while_it_lives_and_remembers_the_last_choice() {
    let root = tempfile::tempdir().unwrap();
    let tmux = Arc::new(FakeTmux::default());
    let uc = support(root.path(), tmux.clone(), Host::allowing(), None);
    let before = uc.status(&alice());
    assert!(!before.alive);
    assert_eq!(before.agent, None);
    assert_eq!(before.last_choice, None);

    uc.open(&alice(), &deep(), None, false).await.unwrap();
    let open = uc.status(&alice());
    assert!(open.alive);
    assert_eq!(open.agent.unwrap().label, "Deep");
    assert_eq!(open.last_choice, Some(deep()));

    // A daemon restarted reads the same record.
    let reread = support(root.path(), tmux.clone(), Host::allowing(), None).status(&alice());
    assert!(reread.alive);
    assert_eq!(reread.last_choice, Some(deep()));

    assert_eq!(uc.close(&alice()), Ok(true));
    let closed = uc.status(&alice());
    assert!(!closed.alive);
    assert_eq!(closed.agent, None);
    assert_eq!(closed.last_choice, Some(deep()), "the last choice stays");
    assert_eq!(uc.close(&alice()), Ok(false), "closing none is no error");
}

#[tokio::test]
async fn a_refusing_extension_stops_every_gesture() {
    let root = tempfile::tempdir().unwrap();
    let tmux = Arc::new(FakeTmux::default());
    let uc = support(root.path(), tmux.clone(), Host::refusing(), None);
    assert!(matches!(
        uc.open(&alice(), &deep(), None, false).await,
        Err(SupportError::Refused(_))
    ));
    assert!(matches!(
        uc.ask(&alice(), "hi"),
        Err(SupportError::Refused(_))
    ));
    assert!(uc.close(&alice()).is_err());
    assert!(tmux.created.lock().unwrap().is_empty());
}
