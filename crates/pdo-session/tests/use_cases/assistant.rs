//! The library assistant: open, focus, save, close.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use pdo_kernel::extension_host::NodeContribution;
use pdo_kernel::{Actor, ResourceKind, Verb};
use pdo_session::application::assistant::{
    AssistantLaunch, CloseAssistant, DeclareFocus, FocusCell, OpenAssistant, OpenAssistantError,
    Opened, SaveFocused, SaveFocusedError,
};
use pdo_session::application::ports::TemplateSaveError;
use pdo_session::domain::launch::LIBASSIST_HOOK_SETTINGS_JSON;
use pdo_session::domain::names::{libassist_session_name_of, LIBASSIST_SESSION_NAME};

use crate::fakes::{FakeTmux, Harness, Host, Templates};

fn launch(root: &Path) -> AssistantLaunch {
    AssistantLaunch {
        repo_root: root.to_path_buf(),
        daemon_port: 6172,
        tmux_cmd_override: Some("true".into()),
    }
}

fn templates(root: &Path, invalid: Option<TemplateSaveError>) -> Arc<Templates> {
    Arc::new(Templates {
        root: root.join(".pdo").join("pipelines"),
        saved: Mutex::new(Vec::new()),
        invalid,
    })
}

#[tokio::test]
async fn the_assistant_opens_once_in_the_templates_folder_with_its_primer_and_hook() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("prompts/builtin")).unwrap();
    std::fs::write(
        root.path().join("prompts/builtin/library-assistant.md"),
        "ROLE",
    )
    .unwrap();
    let tmux = Arc::new(FakeTmux::default());
    let host = Host::allowing();
    let uc = OpenAssistant::new(
        tmux.clone(),
        Arc::new(Harness(pdo_harness::registry::claude())),
        host.clone(),
        launch(root.path()),
    );
    assert_eq!(
        uc.execute(&Actor::LocalOperator).await,
        Ok(Opened {
            session: LIBASSIST_SESSION_NAME.into(),
            created: true
        })
    );
    let (name, cwd, _) = tmux.created.lock().unwrap()[0].clone();
    assert_eq!(name, LIBASSIST_SESSION_NAME);
    assert_eq!(cwd, root.path().join(".pdo/pipelines"));
    let primer = std::fs::read_to_string(root.path().join(".pdo/.libassist/assistant.md")).unwrap();
    assert!(primer.starts_with("# Pipeline Assistant Runtime Preamble"));
    assert!(primer.contains("http://localhost:6172/sessions/libassist/focus"));
    assert!(primer.ends_with("ROLE"));
    assert_eq!(
        std::fs::read_to_string(root.path().join(".pdo/.libassist/settings.json")).unwrap(),
        LIBASSIST_HOOK_SETTINGS_JSON
    );
    assert_eq!(host.asked()[0].resource, ResourceKind::Pipeline);
    assert_eq!(host.asked()[0].verb, Verb::Action("open_assistant".into()));
    assert_eq!(host.asked()[0].target, None);

    let again = uc.execute(&Actor::LocalOperator).await.unwrap();
    assert!(!again.created, "one assistant per Actor");
    assert_eq!(tmux.created.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_harness_whose_hole_takes_no_claude_file_gets_no_hook_file() {
    let root = tempfile::tempdir().unwrap();
    let uc = OpenAssistant::new(
        Arc::new(FakeTmux::default()),
        Arc::new(Harness(pdo_harness::registry::pi())),
        Host::allowing(),
        launch(root.path()),
    );
    uc.execute(&Actor::LocalOperator).await.unwrap();
    assert!(!root.path().join(".pdo/.libassist/settings.json").exists());
}

#[tokio::test]
async fn a_refused_or_failed_open_is_named() {
    let root = tempfile::tempdir().unwrap();
    let tmux = Arc::new(FakeTmux::default());
    let harness = Arc::new(Harness(pdo_harness::registry::claude()));
    let refused = OpenAssistant::new(
        tmux.clone(),
        harness.clone(),
        Host::refusing(),
        launch(root.path()),
    );
    assert!(matches!(
        refused.execute(&Actor::LocalOperator).await,
        Err(OpenAssistantError::Refused(_))
    ));
    *tmux.fail_next_spawn.lock().unwrap() = Some("no server".into());
    let failing = OpenAssistant::new(tmux, harness, Host::allowing(), launch(root.path()));
    assert_eq!(
        failing.execute(&Actor::LocalOperator).await,
        Err(OpenAssistantError::Spawn(
            "failed to open library assistant: no server".into()
        ))
    );
}

#[test]
fn the_focus_names_the_file_the_daemon_resolved() {
    let root = tempfile::tempdir().unwrap();
    let templates = templates(root.path(), None);
    std::fs::create_dir_all(&templates.root).unwrap();
    std::fs::write(templates.root.join("review.yaml"), "name: review").unwrap();
    let focus = Arc::new(FocusCell::default());
    let host = Host::allowing();
    let uc = DeclareFocus::new(focus.clone(), templates.clone(), host.clone());

    let body = uc
        .execute(&Actor::LocalOperator, Some("review".into()))
        .unwrap();
    assert_eq!(body["pipeline_id"], "review");
    assert_eq!(
        body["path"],
        templates
            .root
            .join("review.yaml")
            .to_string_lossy()
            .as_ref()
    );
    assert_eq!(host.asked()[0].resource, ResourceKind::Session);
    assert_eq!(host.asked()[0].verb, Verb::Update);

    // An unsaved template is a legitimate focus, with no file.
    uc.execute(&Actor::LocalOperator, Some("draft".into()))
        .unwrap();
    assert_eq!(focus.get(LIBASSIST_SESSION_NAME).unwrap().path, None);

    // Leaving every edit view clears it.
    let cleared = uc.execute(&Actor::LocalOperator, None).unwrap();
    assert_eq!(
        cleared,
        serde_json::json!({ "ok": true, "pipeline_id": null })
    );
    assert_eq!(focus.get(LIBASSIST_SESSION_NAME), None);
}

#[test]
fn saving_writes_the_focused_template_and_never_a_guess() {
    let root = tempfile::tempdir().unwrap();
    let templates = templates(root.path(), None);
    let focus = Arc::new(FocusCell::default());
    let host = Host::allowing();
    let uc = SaveFocused::new(focus.clone(), templates.clone(), host.clone());
    let prompts: HashMap<String, String> = [("n1".to_string(), "prompt".to_string())].into();

    assert_eq!(
        uc.execute(&Actor::LocalOperator, "name: x", &prompts),
        Err(SaveFocusedError::NoFocus)
    );
    assert!(templates.saved.lock().unwrap().is_empty());

    DeclareFocus::new(focus.clone(), templates.clone(), host.clone())
        .execute(&Actor::LocalOperator, Some("review".into()))
        .unwrap();
    let saved = uc
        .execute(&Actor::LocalOperator, "name: x", &prompts)
        .unwrap();
    assert_eq!(saved.pipeline_id, "review");
    assert_eq!(saved.path, templates.root.join("review.yaml"));
    assert_eq!(
        templates.saved.lock().unwrap().as_slice(),
        [("review".to_string(), "name: x".to_string(), prompts.clone())]
    );
    let save_op = host.asked().last().cloned().unwrap();
    assert_eq!(save_op.resource, ResourceKind::Pipeline);
    assert_eq!(save_op.verb, Verb::Update);
    assert_eq!(save_op.target.as_deref(), Some("review"));
}

#[test]
fn an_invalid_template_or_a_refusal_is_not_written() {
    let root = tempfile::tempdir().unwrap();
    let invalid = TemplateSaveError::Invalid {
        error: "invalid YAML: bad".into(),
        message: "bad".into(),
        line: Some(3),
    };
    let templates = templates(root.path(), Some(invalid.clone()));
    let focus = Arc::new(FocusCell::default());
    DeclareFocus::new(focus.clone(), templates.clone(), Host::allowing())
        .execute(&Actor::LocalOperator, Some("review".into()))
        .unwrap();
    assert_eq!(
        SaveFocused::new(focus.clone(), templates.clone(), Host::allowing()).execute(
            &Actor::LocalOperator,
            "::",
            &HashMap::new()
        ),
        Err(SaveFocusedError::Save(invalid))
    );
    assert!(matches!(
        SaveFocused::new(focus, templates, Host::refusing()).execute(
            &Actor::LocalOperator,
            "name: x",
            &HashMap::new()
        ),
        Err(SaveFocusedError::Refused(_))
    ));
}

#[test]
fn closing_reaps_the_session_and_clears_the_focus_in_one_gesture() {
    let tmux = FakeTmux::with_sessions(&[LIBASSIST_SESSION_NAME]);
    let focus = Arc::new(FocusCell::default());
    focus.set(
        LIBASSIST_SESSION_NAME,
        Some(pdo_session::domain::libassist::LibassistFocus {
            pipeline_id: "review".into(),
            path: None,
            at: chrono::Utc::now(),
        }),
    );
    let host = Host::allowing();
    let uc = CloseAssistant::new(tmux.clone(), focus.clone(), host.clone());
    assert_eq!(uc.execute(&Actor::LocalOperator), Ok(true));
    assert!(!tmux.has(LIBASSIST_SESSION_NAME));
    assert_eq!(focus.get(LIBASSIST_SESSION_NAME), None);
    assert_eq!(host.asked()[0].verb, Verb::Delete);
    assert_eq!(
        uc.execute(&Actor::LocalOperator),
        Ok(false),
        "nothing left to reap"
    );
}

fn person(id: &str) -> Actor {
    Actor::Identified {
        namespace: "test".into(),
        id: id.into(),
    }
}

/// CORE-61: each Actor has its own assistant, launched with what the
/// extensions give that Actor, and announced started by it to the extensions
/// (never on the bus: it belongs to no Run).
#[tokio::test]
async fn two_actors_open_two_assistants_each_with_its_own_environment() {
    let root = tempfile::tempdir().unwrap();
    let tmux = Arc::new(FakeTmux::default());
    let host = Host::allowing();
    *host.contribution.lock().unwrap() = NodeContribution {
        env: [("FIXTURE_HELLO".to_string(), "monde".to_string())].into(),
        preamble: Some("Ligne de fixture".into()),
    };
    let uc = OpenAssistant::new(
        tmux.clone(),
        Arc::new(Harness(pdo_harness::registry::claude())),
        host.clone(),
        launch(root.path()),
    );
    let alice = uc.execute(&person("alice")).await.unwrap();
    let bob = uc.execute(&person("bob")).await.unwrap();
    assert!(alice.created && bob.created);
    assert_ne!(alice.session, bob.session);
    assert_eq!(alice.session, libassist_session_name_of(&person("alice")));
    assert_eq!(tmux.created.lock().unwrap().len(), 2);

    let launches = host.launches.lock().unwrap().clone();
    assert_eq!(launches.len(), 2);
    assert_eq!(launches[0].started_by, Some(person("alice")));
    assert_eq!(launches[1].started_by, Some(person("bob")));
    assert_eq!(launches[1].session.id, bob.session);
    assert_eq!(
        launches[1].session.kind,
        pdo_extension_api::SessionKind::LibraryAssistant
    );
    let (_, _, script) = tmux.created.lock().unwrap()[1].clone();
    assert!(script.contains("FIXTURE_HELLO"), "{script}");
    let primer = std::fs::read_to_string(
        root.path()
            .join(".pdo/.libassist")
            .join(format!("{}.md", bob.session)),
    )
    .unwrap();
    assert!(primer.starts_with("Ligne de fixture\n\n# Pipeline Assistant Runtime Preamble"));

    let events = host.events();
    assert_eq!(events.len(), 2);
    assert!(events.iter().all(|(on_the_bus, _)| !on_the_bus));
    assert_eq!(events[1].1["type"], "session_started");
    assert_eq!(events[1].1["actor"]["id"], "bob");
    assert_eq!(events[1].1["session"]["id"], bob.session.as_str());

    // Re-opening re-attaches, and starts nothing.
    assert!(!uc.execute(&person("alice")).await.unwrap().created);
    assert_eq!(host.events().len(), 2);
}

#[tokio::test]
async fn an_actor_refused_on_the_pipelines_opens_no_assistant() {
    let root = tempfile::tempdir().unwrap();
    let tmux = Arc::new(FakeTmux::default());
    let host = Host::refusing();
    let uc = OpenAssistant::new(
        tmux.clone(),
        Arc::new(Harness(pdo_harness::registry::claude())),
        host.clone(),
        launch(root.path()),
    );
    assert!(matches!(
        uc.execute(&person("reader")).await,
        Err(OpenAssistantError::Refused(_))
    ));
    assert!(tmux.created.lock().unwrap().is_empty());
    assert!(host.launches.lock().unwrap().is_empty());
    assert!(host.events().is_empty());
}

#[test]
fn each_assistant_saves_what_its_own_actor_has_open() {
    let root = tempfile::tempdir().unwrap();
    let templates = templates(root.path(), None);
    let focus = Arc::new(FocusCell::default());
    let host = Host::allowing();
    let declare = DeclareFocus::new(focus.clone(), templates.clone(), host.clone());
    declare
        .execute(&person("alice"), Some("review".into()))
        .unwrap();
    declare
        .execute(&person("bob"), Some("deploy".into()))
        .unwrap();
    assert_eq!(
        host.asked()[1].target.as_deref(),
        Some(libassist_session_name_of(&person("bob")).as_str())
    );

    let save = SaveFocused::new(focus.clone(), templates.clone(), host);
    let saved = save
        .execute(&person("bob"), "name: x", &HashMap::new())
        .unwrap();
    assert_eq!(saved.pipeline_id, "deploy");
    assert_eq!(
        save.execute(&Actor::LocalOperator, "name: x", &HashMap::new()),
        Err(SaveFocusedError::NoFocus),
        "the local operator has nothing open"
    );

    CloseAssistant::new(
        Arc::new(FakeTmux::default()),
        focus.clone(),
        Host::allowing(),
    )
    .execute(&person("alice"))
    .unwrap();
    assert_eq!(
        focus.get(&libassist_session_name_of(&person("alice"))),
        None
    );
    assert!(focus
        .get(&libassist_session_name_of(&person("bob")))
        .is_some());
}
