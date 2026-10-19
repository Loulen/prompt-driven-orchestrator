//! Saving a pipeline from the editor.

use std::collections::HashMap;

use pdo_pipeline::application::ports::Renamed;
use pdo_pipeline::application::PipelineError;

use crate::fakes::*;

fn prompts(entries: &[(&str, &str)]) -> HashMap<String, String> {
    entries
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[tokio::test]
async fn a_save_writes_the_yaml_and_prompts_and_prunes_the_prompts_of_deleted_nodes() {
    let world = World::new();
    world
        .registry
        .put("editable", &pipeline_with_worker("editable"));
    world.registry.put_prompt("editable", "removed", "Stale.");

    let yaml = pipeline_with_worker("editable").replace("version: \"1.0\"", "version: \"2.0\"");
    let saved = world
        .pipelines
        .save(
            &operator(),
            "editable",
            &yaml,
            &prompts(&[("worker", "New.")]),
        )
        .await
        .unwrap();
    assert_eq!((saved.id.as_str(), saved.renamed), ("editable", false));
    let entry = world.registry.get("editable").unwrap();
    assert_eq!(entry.yaml, yaml);
    assert_eq!(entry.prompts, prompts(&[("worker", "New.")]));
    // The watcher suppresses the daemon's own write: the save announces it
    // once, for the lists to read its new version (ADR-0088).
    assert_eq!(world.events.0.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_save_refuses_an_unknown_pipeline_and_a_yaml_that_does_not_parse() {
    let world = World::new();
    assert!(matches!(
        world
            .pipelines
            .save(
                &operator(),
                "ghost",
                &pipeline_yaml("ghost"),
                &HashMap::new()
            )
            .await,
        Err(PipelineError::NotFound)
    ));
    world.registry.put("bad-save", &pipeline_yaml("bad-save"));
    for broken in ["name: [unclosed", "version: \"1.0\"\nnodes: []\n"] {
        assert!(matches!(
            world
                .pipelines
                .save(&operator(), "bad-save", broken, &HashMap::new())
                .await,
            Err(PipelineError::Invalid(_))
        ));
    }
    assert_eq!(
        world.registry.get("bad-save").unwrap().yaml,
        pipeline_yaml("bad-save")
    );
}

#[tokio::test]
async fn a_save_that_changes_the_name_moves_the_entry_and_records_the_rename() {
    let world = World::new();
    world
        .registry
        .put("saved-old", &pipeline_with_worker("saved-old"));
    world.registry.put_prompt("saved-old", "worker", "Kept.");

    let saved = world
        .pipelines
        .save(
            &operator(),
            "saved-old",
            &pipeline_with_worker("Saved New"),
            &prompts(&[("worker", "Edited.")]),
        )
        .await
        .unwrap();
    assert_eq!((saved.id.as_str(), saved.renamed), ("saved-new", true));
    assert!(!world.registry.exists_id("saved-old"));
    assert_eq!(
        world.registry.get("saved-new").unwrap().prompts["worker"],
        "Edited."
    );
    assert_eq!(
        *world.renames.0.lock().unwrap(),
        [Renamed {
            old_id: "saved-old".into(),
            old_name: "saved-old".into(),
            new_id: "saved-new".into(),
            new_name: "Saved New".into(),
        }]
    );
    assert_eq!(
        *world.events.0.lock().unwrap(),
        [(
            "saved-new".to_string(),
            "/registry/saved-new.yaml".to_string()
        )]
    );
}

#[tokio::test]
async fn a_name_changing_save_refuses_a_collision_and_keeps_the_old_file() {
    let world = World::new();
    world.registry.put("moving", &pipeline_yaml("moving"));
    world.registry.put("target", &pipeline_yaml("target"));
    assert!(matches!(
        world
            .pipelines
            .save(
                &operator(),
                "moving",
                &pipeline_yaml("Target"),
                &HashMap::new()
            )
            .await,
        Err(PipelineError::NameTaken(_))
    ));
    assert_eq!(
        world.registry.get("moving").unwrap().yaml,
        pipeline_yaml("moving")
    );
}

/// #886 — a stem that is not the slug of its name (`Mixed-Case`) is not moved
/// by a save that keeps the name, even while a Run uses it; one that really
/// changes the name still moves it.
#[tokio::test]
async fn only_a_save_that_changes_the_name_moves_a_mixed_case_stem() {
    let world = World::new();
    world
        .registry
        .put("Mixed-Case", &pipeline_yaml("Mixed-Case"));
    world.registry.put_prompt("Mixed-Case", "start", "Kept.");
    world.busy("Mixed-Case", 1);
    let yaml = pipeline_yaml("Mixed-Case").replace("\"1.0\"", "\"1.1\"");
    let saved = world
        .pipelines
        .save(&operator(), "Mixed-Case", &yaml, &HashMap::new())
        .await
        .unwrap();
    assert!(!saved.renamed);
    assert_eq!(world.registry.ids(), ["Mixed-Case"]);
    assert_eq!(
        world.registry.get("Mixed-Case").unwrap().prompts["start"],
        "Kept."
    );

    world.busy("Mixed-Case", 0);
    let saved = world
        .pipelines
        .save(
            &operator(),
            "Mixed-Case",
            &pipeline_yaml("Other Name"),
            &HashMap::new(),
        )
        .await
        .unwrap();
    assert_eq!(saved.id, "other-name");
    assert_eq!(world.registry.ids(), ["other-name"]);
}
