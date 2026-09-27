//! Creating, duplicating and deleting a pipeline.

use pdo_pipeline::application::PipelineError;
use pdo_pipeline::domain::pipeline::parse_pipeline;

use crate::fakes::*;

#[test]
fn a_new_pipeline_is_a_start_and_an_end_named_after_the_request() {
    let world = World::new();
    let created = world.pipelines.create(&operator(), "  My Pipe  ").unwrap();
    assert_eq!(created.id, "my-pipe");
    let yaml = world.registry.get("my-pipe").unwrap().yaml;
    let parsed = parse_pipeline(&yaml).unwrap().pipeline;
    assert_eq!(parsed.name, "My Pipe");
    let ids: Vec<&str> = parsed.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, ["start", "end"]);
}

#[test]
fn a_name_that_needs_quoting_still_scaffolds_a_parsable_file() {
    let world = World::new();
    let created = world.pipelines.create(&operator(), "my: name").unwrap();
    let yaml = world.registry.get(&created.id).unwrap().yaml;
    assert_eq!(parse_pipeline(&yaml).unwrap().pipeline.name, "my: name");
}

#[test]
fn creation_refuses_a_blank_name_a_taken_stem_and_a_taken_visible_name() {
    let world = World::new();
    assert!(matches!(
        world.pipelines.create(&operator(), "   "),
        Err(PipelineError::NameRequired)
    ));
    world.pipelines.create(&operator(), "Dup").unwrap();
    assert!(matches!(
        world.pipelines.create(&operator(), "Dup"),
        Err(PipelineError::AlreadyExists)
    ));
    // Another stem already carries the visible name.
    world.registry.put("x", &pipeline_yaml("Foo Bar"));
    assert!(matches!(
        world.pipelines.create(&operator(), "Foo Bar"),
        Err(PipelineError::AlreadyExists)
    ));
    assert!(!world.registry.exists_id("foo-bar"));
}

#[test]
fn a_duplicate_takes_the_first_free_copy_name_and_carries_the_live_prompts() {
    let world = World::new();
    world
        .registry
        .put("review", &pipeline_with_worker("review"));
    world.registry.put_prompt("review", "worker", "Work.");
    world.registry.put_prompt("review", "orphan", "Left over.");

    let first = world.pipelines.duplicate(&operator(), "review").unwrap();
    assert_eq!(first.id, "review-copy");
    let copy = world.registry.get("review-copy").unwrap();
    assert_eq!(
        parse_pipeline(&copy.yaml).unwrap().pipeline.name,
        "review (copy)"
    );
    assert_eq!(copy.prompts.len(), 1);

    let second = world
        .pipelines
        .duplicate(&operator(), "review-copy")
        .unwrap();
    assert_eq!(second.id, "review-copy-2");
    let yaml = world.registry.get("review-copy-2").unwrap().yaml;
    assert_eq!(
        parse_pipeline(&yaml).unwrap().pipeline.name,
        "review (copy 2)"
    );
}

#[test]
fn a_duplicate_of_an_unknown_or_broken_pipeline_is_refused() {
    let world = World::new();
    assert!(matches!(
        world.pipelines.duplicate(&operator(), "ghost"),
        Err(PipelineError::NotFound)
    ));
    world.registry.put("broken", "nodes: [");
    assert!(matches!(
        world.pipelines.duplicate(&operator(), "broken"),
        Err(PipelineError::Invalid(_))
    ));
}

#[tokio::test]
async fn a_delete_removes_the_pipeline_unless_a_run_in_flight_uses_it() {
    let world = World::new();
    world.registry.put("busy", &pipeline_yaml("busy"));
    world.busy("busy", 2);
    assert!(matches!(
        world.pipelines.delete(&operator(), "busy").await,
        Err(PipelineError::ActiveRuns(2))
    ));
    assert!(world.registry.exists_id("busy"));

    // A completed or archived Run does not count (the Runs port says 0).
    world.busy("busy", 0);
    world.pipelines.delete(&operator(), "busy").await.unwrap();
    assert!(!world.registry.exists_id("busy"));
    assert!(matches!(
        world.pipelines.delete(&operator(), "busy").await,
        Err(PipelineError::NotFound)
    ));
}
