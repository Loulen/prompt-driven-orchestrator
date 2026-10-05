//! The node library: star a node, instantiate it, take it out.

use pdo_pipeline::application::PipelineError;
use pdo_pipeline::domain::library::LibraryEntry;
use pdo_pipeline::domain::pipeline::NodeType;

use crate::fakes::*;

fn entry(name: &str, node_type: NodeType, isolated: Option<bool>) -> LibraryEntry {
    LibraryEntry {
        name: name.into(),
        node_type,
        inputs: vec![],
        outputs: vec![],
        interactive: false,
        model: None,
        effort: None,
        max_iter: None,
        branches: None,
        prompt: "You are a reviewer.".into(),
        isolated_worktree: isolated,
    }
}

#[test]
fn a_starred_node_is_listed_instantiated_and_deleted_once() {
    let world = World::new();
    assert!(world.pipelines.library().is_empty());
    let saved = world
        .pipelines
        .save_to_library(&operator(), entry("Reviewer", NodeType::Agent, Some(false)))
        .unwrap();
    assert_eq!(saved.prompt, "You are a reviewer.");
    assert_eq!(world.pipelines.library().len(), 1);
    let found = world.pipelines.library_entry("Reviewer").unwrap();
    assert_eq!(found.isolated_worktree, Some(false));

    world
        .pipelines
        .delete_from_library(&operator(), "Reviewer")
        .unwrap();
    assert!(world.pipelines.library_entry("Reviewer").is_none());
    assert!(matches!(
        world.pipelines.delete_from_library(&operator(), "Reviewer"),
        Err(PipelineError::NotFound)
    ));
}

/// #655 — the entry always states where it works: a silent Agent is stamped
/// with its type's default, a Switch carries no isolation at all.
#[test]
fn the_library_states_each_entrys_isolation() {
    let world = World::new();
    let agent = world
        .pipelines
        .save_to_library(&operator(), entry("Silent", NodeType::Agent, None))
        .unwrap();
    assert_eq!(agent.isolated_worktree, Some(true));
    let script = world
        .pipelines
        .save_to_library(&operator(), entry("Script", NodeType::Script, None))
        .unwrap();
    assert_eq!(script.isolated_worktree, Some(false));
    let switch = world
        .pipelines
        .save_to_library(&operator(), entry("Router", NodeType::Switch, Some(true)))
        .unwrap();
    assert_eq!(switch.isolated_worktree, None);
    assert_eq!(
        world
            .pipelines
            .library_entry("Silent")
            .unwrap()
            .isolated_worktree,
        Some(true)
    );
}
