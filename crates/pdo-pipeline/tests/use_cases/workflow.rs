//! Importing a Claude Code workflow `.js` (#155 / ADR-0016).

use pdo_pipeline::application::WorkflowImportError;
use pdo_pipeline::domain::pipeline::parse_pipeline;

use crate::fakes::*;

const WORKFLOW: &str = r#"agent(`Fix the failing test.`, { label: 'implement' })"#;

#[test]
fn a_workflow_becomes_a_draft_pipeline_in_the_registry() {
    let world = World::new();
    let imported = world
        .pipelines
        .import_workflow(&operator(), "flow.js", WORKFLOW)
        .unwrap();
    // No `meta.name`: the draft is named after the file.
    assert_eq!(imported.id, "flow");
    let entry = world.registry.get("flow").unwrap();
    let parsed = parse_pipeline(&entry.yaml).unwrap().pipeline;
    assert_eq!(parsed.name, "flow");
    assert_eq!(parsed.nodes.len(), 3, "{:?}", parsed.nodes);
    assert!(!entry.prompts.is_empty());

    // A second import of the same workflow takes the next free identity.
    let again = world
        .pipelines
        .import_workflow(&operator(), "flow.js", WORKFLOW)
        .unwrap();
    assert_eq!(again.id, "flow-2");
}

#[test]
fn a_source_the_importer_cannot_read_is_refused_and_writes_nothing() {
    let world = World::new();
    assert!(matches!(
        world
            .pipelines
            .import_workflow(&operator(), "bad.js", "this is { not javascript"),
        Err(WorkflowImportError::Untranslatable(_))
    ));
    assert!(world.registry.ids().is_empty());
}
