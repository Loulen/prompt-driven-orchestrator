//! The versions of a pipeline (ADR-0088): Save freezes the draft as a
//! numbered, titled version, or replaces the last one, and may keep the
//! version saved until now as a pipeline of its own. The file is always the
//! last version.

use std::collections::HashMap;

use pdo_kernel::{Operation, ResourceKind, Verb};
use pdo_pipeline::application::PipelineError;
use pdo_pipeline::domain::draft::DraftContent;
use pdo_pipeline::domain::version::SaveAs;

use crate::fakes::*;

fn world_with(id: &str, yaml: &str) -> World {
    let world = World::new();
    world.registry.put(id, yaml);
    world
}

async fn save(
    world: &World,
    id: &str,
    yaml: &str,
    save_as: SaveAs,
) -> pdo_pipeline::application::SaveOutcome {
    world
        .pipelines
        .save_draft(
            &operator(),
            id,
            yaml,
            &HashMap::new(),
            &DraftContent::default(),
            &save_as,
        )
        .await
        .unwrap()
}

fn titled(title: &str) -> SaveAs {
    SaveAs {
        title: Some(title.into()),
        ..SaveAs::new_version()
    }
}

fn replacing() -> SaveAs {
    SaveAs {
        new_version: false,
        ..SaveAs::new_version()
    }
}

#[tokio::test]
async fn the_file_is_v1_titled_by_its_free_version_field() {
    let world = world_with("flow", &pipeline_yaml("flow"));
    let listed = world.pipelines.versions("flow").await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(
        (listed[0].meta.number, listed[0].meta.title.as_str()),
        (1, "1.0")
    );
    assert_eq!(listed[0].meta.author, None);
    let plain = world_with("bare", "name: bare\nnodes: []\nedges: []\n");
    assert_eq!(
        plain.pipelines.versions("bare").await.unwrap()[0]
            .meta
            .title,
        "v1"
    );
}

#[tokio::test]
async fn a_save_is_the_next_version_with_its_title_and_author() {
    let world = world_with("flow", &pipeline_yaml("flow"));
    let saved = save(
        &world,
        "flow",
        &pipeline_with_worker("flow"),
        SaveAs::new_version(),
    )
    .await;
    assert_eq!(
        (saved.version.number, saved.version.title.as_str()),
        (2, "v2")
    );
    let saved = save(
        &world,
        "flow",
        &pipeline_yaml("flow"),
        titled("Drop the worker"),
    )
    .await;
    assert_eq!(saved.version.number, 3);
    assert_eq!(
        world.versions.titles("flow"),
        [
            (1, "1.0".to_string()),
            (2, "v2".into()),
            (3, "Drop the worker".into())
        ]
    );
    let two = world.versions.of("flow", 2).unwrap();
    assert_eq!(two.yaml, pipeline_with_worker("flow"));
    assert_eq!(two.meta.author, Some(operator()));
    // The file is the last version.
    assert_eq!(
        world.registry.get("flow").unwrap().yaml,
        pipeline_yaml("flow")
    );
}

#[tokio::test]
async fn a_replacement_keeps_the_number() {
    let world = world_with("flow", &pipeline_yaml("flow"));
    save(&world, "flow", &pipeline_yaml("flow"), titled("First pass")).await;
    let saved = save(&world, "flow", &pipeline_with_worker("flow"), replacing()).await;
    assert_eq!(
        (saved.version.number, saved.version.title.as_str()),
        (2, "First pass")
    );
    assert_eq!(world.versions.titles("flow").len(), 2);
    assert_eq!(
        world.versions.of("flow", 2).unwrap().yaml,
        pipeline_with_worker("flow")
    );
}

#[tokio::test]
async fn a_replacement_is_dated_by_its_save() {
    let world = world_with("flow", &pipeline_yaml("flow"));
    save(&world, "flow", &pipeline_yaml("flow"), titled("First pass")).await;
    let first = world.pipelines.versions("flow").await.unwrap()[0]
        .meta
        .saved_at
        .clone();
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    save(&world, "flow", &pipeline_with_worker("flow"), replacing()).await;
    let listed = world.pipelines.versions("flow").await.unwrap();
    assert_eq!(listed[0].meta.number, 2);
    assert!(
        listed[0].meta.saved_at > first,
        "{} after {first}",
        listed[0].meta.saved_at
    );
}

#[tokio::test]
async fn keep_old_makes_the_saved_version_a_pipeline_of_its_own() {
    let world = world_with("flow", &pipeline_yaml("flow"));
    world.registry.put_prompt("flow", "start", "Kept prompt.");
    let saved = save(
        &world,
        "flow",
        &pipeline_with_worker("flow"),
        SaveAs {
            keep_old: true,
            ..replacing()
        },
    )
    .await;
    let kept = saved.kept.unwrap();
    let entry = world.registry.get(&kept.id).unwrap();
    assert!(entry.yaml.contains("name: flow (v1)"), "{}", entry.yaml);
    assert!(!entry.yaml.contains("worker"));
    assert_eq!(
        entry.prompts.get("start").map(String::as_str),
        Some("Kept prompt.")
    );
    // Its history starts with the version it keeps, its title included.
    assert_eq!(world.versions.titles(&kept.id), [(1, "1.0".to_string())]);
    // The pipeline saved replaced v1.
    assert_eq!(saved.version.number, 1);
    assert!(world.registry.get("flow").unwrap().yaml.contains("worker"));
    assert!(world
        .host
        .asked()
        .contains(&Operation::new(ResourceKind::Pipeline, Verb::Create)));
}

#[tokio::test]
async fn a_version_that_cannot_be_kept_fails_the_save_with_nothing_written() {
    let world = World::new();
    world.registry.put("flow", &pipeline_yaml("flow"));
    // v1 holds a document that no longer parses: it cannot become a pipeline.
    world.pipelines.versions("flow").await.unwrap();
    let mut broken = world.versions.of("flow", 1).unwrap();
    broken.yaml = "name: [unclosed".into();
    world
        .versions
        .0
        .lock()
        .unwrap()
        .get_mut("flow")
        .unwrap()
        .insert(1, broken);
    let refused = world
        .pipelines
        .save_draft(
            &operator(),
            "flow",
            &pipeline_with_worker("flow"),
            &HashMap::new(),
            &DraftContent::default(),
            &SaveAs {
                keep_old: true,
                ..SaveAs::new_version()
            },
        )
        .await;
    assert!(matches!(refused, Err(PipelineError::Invalid(_))));
    assert_eq!(
        world.registry.get("flow").unwrap().yaml,
        pipeline_yaml("flow")
    );
    assert_eq!(world.versions.titles("flow").len(), 1);
}

#[tokio::test]
async fn a_save_in_place_is_announced_with_its_new_version() {
    let world = world_with("flow", &pipeline_yaml("flow"));
    save(
        &world,
        "flow",
        &pipeline_with_worker("flow"),
        SaveAs::new_version(),
    )
    .await;
    assert_eq!(*world.events.3.lock().unwrap(), ["flow"]);
    // No editor reloads the file it saved.
    assert!(world.events.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_version_reads_back_as_the_editor_shows_it() {
    let world = world_with("flow", &pipeline_yaml("flow"));
    save(
        &world,
        "flow",
        &pipeline_with_worker("flow"),
        SaveAs::new_version(),
    )
    .await;
    let first = world.pipelines.version("flow", 1).await.unwrap();
    assert_eq!(first.meta.title, "1.0");
    assert_eq!(first.pipeline["nodes"].as_array().unwrap().len(), 2);
    let second = world.pipelines.version("flow", 2).await.unwrap();
    assert_eq!(second.pipeline["nodes"].as_array().unwrap().len(), 3);
    assert!(matches!(
        world.pipelines.version("flow", 9).await,
        Err(PipelineError::NotFound)
    ));
}

#[tokio::test]
async fn the_history_is_newest_first_with_the_runs_of_each_version() {
    let world = world_with("flow", &pipeline_yaml("flow"));
    save(
        &world,
        "flow",
        &pipeline_with_worker("flow"),
        SaveAs::new_version(),
    )
    .await;
    world.launched("flow", 1, 4);
    let listed = world.pipelines.versions("flow").await.unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|v| (v.meta.number, v.runs))
            .collect::<Vec<_>>(),
        [(2, 0), (1, 4)]
    );
    assert_eq!(
        world.pipelines.last_version("flow").await.unwrap().number,
        2
    );
    assert_eq!(world.pipelines.last_version("ghost").await, None);
}

#[tokio::test]
async fn the_versions_follow_a_rename_and_go_with_a_delete() {
    let world = world_with("flow", &pipeline_yaml("flow"));
    save(
        &world,
        "flow",
        &pipeline_with_worker("flow"),
        SaveAs::new_version(),
    )
    .await;
    world
        .pipelines
        .rename(&operator(), "flow", "stream")
        .await
        .unwrap();
    assert!(world.versions.titles("flow").is_empty());
    assert_eq!(world.versions.titles("stream").len(), 2);
    // The last version is the file, its new name included.
    assert!(world
        .versions
        .of("stream", 2)
        .unwrap()
        .yaml
        .contains("name: stream"));
    world.pipelines.delete(&operator(), "stream").await.unwrap();
    assert!(world.versions.titles("stream").is_empty());
}

#[tokio::test]
async fn a_save_that_renames_carries_the_versions() {
    let world = world_with("flow", &pipeline_yaml("flow"));
    let saved = save(
        &world,
        "flow",
        &pipeline_yaml("stream"),
        SaveAs::new_version(),
    )
    .await;
    assert_eq!(saved.id, "stream");
    assert_eq!(world.versions.titles("stream").len(), 2);
    assert!(world.versions.titles("flow").is_empty());
}

#[tokio::test]
async fn an_overwrite_is_a_new_version() {
    let world = world_with("flow", &pipeline_yaml("flow"));
    world
        .pipelines
        .overwrite(
            &operator(),
            "flow",
            &pipeline_with_worker("flow"),
            &HashMap::new(),
        )
        .await
        .unwrap();
    assert_eq!(
        world.versions.titles("flow"),
        [(1, "1.0".to_string()), (2, "v2".into())]
    );
    assert_eq!(
        world.versions.of("flow", 2).unwrap().yaml,
        pipeline_with_worker("flow")
    );
}

#[tokio::test]
async fn a_refused_save_records_no_version() {
    let world = World::refusing("read only");
    world.registry.put("flow", &pipeline_yaml("flow"));
    let refused = world
        .pipelines
        .save_draft(
            &operator(),
            "flow",
            &pipeline_with_worker("flow"),
            &HashMap::new(),
            &DraftContent::default(),
            &SaveAs::new_version(),
        )
        .await;
    assert!(matches!(refused, Err(PipelineError::Denied(_))));
    assert!(world
        .versions
        .titles("flow")
        .iter()
        .all(|(number, _)| *number == 1));
}
