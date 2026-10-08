//! Every mutation asks the Autorisation d'opération first; a refusal touches
//! nothing. Reads never ask.

use std::collections::HashMap;

use pdo_kernel::{Operation, ResourceKind, Verb};
use pdo_pipeline::application::PipelineError;
use pdo_pipeline::domain::library::LibraryEntry;
use pdo_pipeline::domain::pipeline::NodeType;

use crate::fakes::*;

fn denied<T: std::fmt::Debug>(result: Result<T, PipelineError>) {
    match result {
        Err(PipelineError::Denied(denied)) => assert_eq!(denied.reason, "read only"),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn a_refused_mutation_changes_nothing() {
    let world = World::refusing("read only");
    world.registry.put("kept", &pipeline_yaml("kept"));
    let before = world.registry.get("kept").unwrap().yaml;

    denied(world.pipelines.create(&operator(), "new"));
    denied(world.pipelines.duplicate(&operator(), "kept"));
    denied(
        world
            .pipelines
            .save(&operator(), "kept", &pipeline_yaml("kept"), &HashMap::new())
            .await,
    );
    denied(world.pipelines.rename(&operator(), "kept", "Other").await);
    denied(world.pipelines.delete(&operator(), "kept").await);
    denied(world.pipelines.import_document(&operator(), "x", &[]).await);
    denied(world.pipelines.save_to_library(
        &operator(),
        LibraryEntry {
            on_demand: false,
            name: "n".into(),
            node_type: NodeType::Agent,
            inputs: vec![],
            outputs: vec![],
            interactive: false,
            model: None,
            effort: None,
            max_iter: None,
            branches: None,
            prompt: String::new(),
            isolated_worktree: None,
        },
    ));
    denied(world.pipelines.delete_from_library(&operator(), "n"));
    assert!(matches!(
        world.pipelines.import_workflow(&operator(), "f.js", "x"),
        Err(pdo_pipeline::application::WorkflowImportError::Pipeline(
            PipelineError::Denied(_)
        ))
    ));

    assert_eq!(world.registry.ids(), ["kept"]);
    assert_eq!(world.registry.get("kept").unwrap().yaml, before);
    assert!(world.library.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn each_mutation_names_its_operation_and_reads_ask_nothing() {
    let world = World::new();
    world.registry.put("p", &pipeline_yaml("p"));
    world.pipelines.list().unwrap();
    world.pipelines.detail("p").unwrap();
    world.pipelines.export_document("p").unwrap();
    assert!(world.host.asked().is_empty());

    world.pipelines.create(&operator(), "q").unwrap();
    world
        .pipelines
        .rename(&operator(), "p", "P2")
        .await
        .unwrap();
    world.pipelines.delete(&operator(), "q").await.unwrap();
    assert_eq!(
        world.host.asked(),
        [
            Operation::new(ResourceKind::Pipeline, Verb::Create),
            Operation::new(ResourceKind::Pipeline, Verb::Update).on("p"),
            Operation::new(ResourceKind::Pipeline, Verb::Delete).on("q"),
        ]
    );
}
