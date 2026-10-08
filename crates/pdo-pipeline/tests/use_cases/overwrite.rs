//! Overwriting a shared pipeline with a Run's whole snapshot (ADR-0085), and
//! the fingerprint the warning compares.

use std::collections::HashMap;

use pdo_kernel::{Operation, ResourceKind, Verb};
use pdo_pipeline::application::PipelineError;

use crate::fakes::*;

fn prompts(entries: &[(&str, &str)]) -> HashMap<String, String> {
    entries
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[tokio::test]
async fn an_overwrite_copies_the_snapshot_prunes_orphan_prompts_and_announces_it() {
    let world = World::new();
    world.registry.put("shared", &pipeline_with_worker("shared"));
    world.registry.put_prompt("shared", "worker", "Old.");
    world.registry.put_prompt("shared", "dropped", "A node the snapshot no longer has.");

    // The snapshot renamed the pipeline: the id stays, nothing moves.
    let snapshot = pipeline_with_worker("Renamed in the run");
    world
        .pipelines
        .overwrite(&operator(), "shared", &snapshot, &prompts(&[("worker", "New.")]))
        .unwrap();

    assert_eq!(world.registry.ids(), ["shared"]);
    let entry = world.registry.get("shared").unwrap();
    assert_eq!(entry.yaml, snapshot);
    assert_eq!(entry.prompts, prompts(&[("worker", "New.")]));
    let announced = world.events.0.lock().unwrap().clone();
    assert_eq!(announced.len(), 1);
    assert_eq!(announced[0].0, "shared");
    assert_eq!(
        world.host.asked(),
        [Operation::new(ResourceKind::Pipeline, Verb::Update).on("shared")]
    );
}

#[tokio::test]
async fn an_overwrite_refuses_a_missing_pipeline_an_invalid_snapshot_and_a_denied_update() {
    let world = World::new();
    assert!(matches!(
        world
            .pipelines
            .overwrite(&operator(), "ghost", &pipeline_yaml("ghost"), &HashMap::new()),
        Err(PipelineError::NotFound)
    ));
    world.registry.put("kept", &pipeline_yaml("kept"));
    assert!(matches!(
        world
            .pipelines
            .overwrite(&operator(), "kept", "name: [unclosed", &HashMap::new()),
        Err(PipelineError::Invalid(_))
    ));

    let refused = World::refusing("read only");
    refused.registry.put("kept", &pipeline_yaml("kept"));
    assert!(matches!(
        refused
            .pipelines
            .overwrite(&operator(), "kept", &pipeline_with_worker("kept"), &HashMap::new()),
        Err(PipelineError::Denied(_))
    ));
    for w in [&world, &refused] {
        assert_eq!(w.registry.get("kept").unwrap().yaml, pipeline_yaml("kept"));
        assert!(w.events.0.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn the_fingerprint_moves_with_the_yaml_and_the_prompts_and_is_none_for_a_missing_pipeline() {
    let world = World::new();
    assert_eq!(world.pipelines.fingerprint("ghost"), None);
    world.registry.put("p", &pipeline_with_worker("p"));
    let first = world.pipelines.fingerprint("p").unwrap();
    assert_eq!(world.pipelines.fingerprint("p").as_deref(), Some(first.as_str()));

    world.registry.put_prompt("p", "worker", "Edited.");
    let after_prompt = world.pipelines.fingerprint("p").unwrap();
    assert_ne!(first, after_prompt);

    world.registry.put("p", &pipeline_yaml("p"));
    assert_ne!(after_prompt, world.pipelines.fingerprint("p").unwrap());
    // Reads ask nothing.
    assert!(world.host.asked().is_empty());
}
