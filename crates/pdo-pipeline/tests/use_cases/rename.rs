//! Renaming a pipeline (#774): the visible name and the file stem move
//! together, with the refusals that keep them 1:1.

use pdo_pipeline::application::ports::Renamed;
use pdo_pipeline::application::{PipelineError, RenameOutcome};
use pdo_pipeline::domain::pipeline::parse_pipeline;

use crate::fakes::*;

#[tokio::test]
async fn a_rename_moves_the_file_and_its_prompts_splices_the_name_and_records_it() {
    let world = World::new();
    let yaml = format!(
        "# keep this comment\n{}custom_key: kept\n",
        pipeline_with_worker("old-name")
    );
    world.registry.put("old-name", &yaml);
    world.registry.put_prompt("old-name", "worker", "Prompt.");

    let outcome = world
        .pipelines
        .rename(&operator(), "old-name", "New Name")
        .await
        .unwrap();
    assert_eq!(
        outcome,
        RenameOutcome::Moved {
            id: "new-name".into(),
            name: "New Name".into(),
            path: "/registry/new-name.yaml".into(),
        }
    );
    assert!(!world.registry.exists_id("old-name"));
    let moved = world.registry.get("new-name").unwrap();
    assert!(moved
        .yaml
        .starts_with("# keep this comment\nname: New Name\n"));
    assert!(moved.yaml.contains("custom_key: kept"));
    assert_eq!(moved.prompts["worker"], "Prompt.");
    assert_eq!(
        *world.renames.0.lock().unwrap(),
        [Renamed {
            old_id: "old-name".into(),
            old_name: "old-name".into(),
            new_id: "new-name".into(),
            new_name: "New Name".into(),
        }]
    );
    assert_eq!(
        *world.events.0.lock().unwrap(),
        [(
            "new-name".to_string(),
            "/registry/new-name.yaml".to_string()
        )]
    );
}

#[tokio::test]
async fn renaming_to_the_current_name_is_a_no_op() {
    let world = World::new();
    world.registry.put("steady", &pipeline_yaml("steady"));
    let outcome = world
        .pipelines
        .rename(&operator(), "steady", " steady ")
        .await
        .unwrap();
    assert_eq!(
        outcome,
        RenameOutcome::Unchanged {
            id: "steady".into(),
            name: "steady".into()
        }
    );
    assert!(world.events.0.lock().unwrap().is_empty());
    assert!(world.renames.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_display_only_rename_splices_the_name_and_records_nothing() {
    let world = World::new();
    world.registry.put("foo-bar", &pipeline_yaml("foo-bar"));
    world.busy("foo-bar", 1);
    let outcome = world
        .pipelines
        .rename(&operator(), "foo-bar", "foo bar")
        .await
        .unwrap();
    assert_eq!(
        outcome,
        RenameOutcome::InPlace {
            id: "foo-bar".into(),
            name: "foo bar".into()
        }
    );
    let yaml = world.registry.get("foo-bar").unwrap().yaml;
    assert_eq!(parse_pipeline(&yaml).unwrap().pipeline.name, "foo bar");
    assert!(world.renames.0.lock().unwrap().is_empty());
    assert_eq!(world.events.0.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_rename_refuses_a_taken_stem_a_taken_name_and_a_run_in_flight() {
    let world = World::new();
    world.registry.put("first", &pipeline_yaml("first"));
    world
        .registry
        .put("collider-b", &pipeline_yaml("something else"));
    world.registry.put("other", &pipeline_yaml("Taken Name"));

    match world
        .pipelines
        .rename(&operator(), "first", "collider b")
        .await
    {
        Err(PipelineError::NameTaken(reason)) => {
            assert_eq!(reason, "a pipeline file 'collider-b.yaml' already exists")
        }
        other => panic!("{other:?}"),
    }
    match world
        .pipelines
        .rename(&operator(), "first", "Taken Name")
        .await
    {
        Err(PipelineError::NameTaken(reason)) => {
            assert_eq!(reason, "a pipeline already uses the name \"Taken Name\"")
        }
        other => panic!("{other:?}"),
    }
    world.busy("first", 1);
    assert!(matches!(
        world.pipelines.rename(&operator(), "first", "Fresh").await,
        Err(PipelineError::ActiveRuns(1))
    ));
    // Once the Run completes, the rename goes through.
    world.busy("first", 0);
    world
        .pipelines
        .rename(&operator(), "first", "Fresh")
        .await
        .unwrap();
    assert!(world.registry.exists_id("fresh"));
}

#[tokio::test]
async fn a_rename_refuses_a_blank_name_and_an_unknown_pipeline() {
    let world = World::new();
    assert!(matches!(
        world.pipelines.rename(&operator(), "ghost", "x").await,
        Err(PipelineError::NotFound)
    ));
    world.registry.put("quiet", &pipeline_yaml("quiet"));
    assert!(matches!(
        world.pipelines.rename(&operator(), "quiet", "  ").await,
        Err(PipelineError::NameRequired)
    ));
}
