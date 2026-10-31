//! The validation of a pipeline (CONTEXT.md « Pipeline validé »): every
//! Projet until stated, a test pipeline once created, duplicated or imported,
//! carried by a rename, forgotten with the pipeline, withdrawn with a Projet.

use std::collections::HashMap;

use pdo_kernel::{Operation, ResourceKind, Verb};
use pdo_pipeline::application::PipelineError;
use pdo_pipeline::domain::validation::PipelineValidation;

use crate::fakes::*;

fn projects(ids: &[&str]) -> PipelineValidation {
    PipelineValidation::Projects {
        project_ids: ids.iter().map(|id| id.to_string()).collect(),
    }
}

fn all_listed_first_for(ids: &[&str]) -> PipelineValidation {
    PipelineValidation::All {
        project_ids: ids.iter().map(|id| id.to_string()).collect(),
    }
}

#[tokio::test]
async fn a_pipeline_that_states_nothing_is_validated_for_every_projet() {
    let world = World::new();
    world.registry.put("legacy", &pipeline_yaml("legacy"));
    assert_eq!(world.pipelines.validations().await.get("legacy"), None);
    assert_eq!(PipelineValidation::default(), PipelineValidation::all());
}

#[tokio::test]
async fn created_duplicated_and_imported_pipelines_are_born_test_pipelines() {
    let world = World::new();
    world.registry.put("source", &pipeline_yaml("source"));

    let created = world.pipelines.create(&operator(), "Fresh").unwrap();
    world
        .pipelines
        .settle_created(&operator(), &created.id)
        .await;
    let copy = world.pipelines.duplicate(&operator(), "source").unwrap();
    world.pipelines.settle_created(&operator(), &copy.id).await;
    let document = world.pipelines.export_document("source").unwrap();
    let imported = world
        .pipelines
        .import_document(&operator(), &document, &[])
        .await
        .unwrap();
    world
        .pipelines
        .settle_created(&operator(), &imported.id)
        .await;

    for id in [&created.id, &copy.id, &imported.id] {
        assert_eq!(
            world.validations.of(id),
            Some(PipelineValidation::None),
            "{id}"
        );
    }
    assert_eq!(
        world.validations.of("source"),
        None,
        "the source keeps its own"
    );
    // Born a test pipeline is a validation that changed: an open launch form
    // and the other clients read the list again.
    assert_eq!(
        world.events.validations_changed(),
        [created.id.clone(), copy.id.clone(), imported.id.clone()]
    );
}

#[tokio::test]
async fn a_deleted_pipeline_announces_its_validation_gone() {
    let world = World::new();
    world.registry.put("old", &pipeline_yaml("old"));
    world.pipelines.delete(&operator(), "old").await.unwrap();
    assert_eq!(world.events.validations_changed(), ["old"]);
}

#[tokio::test]
async fn validating_replaces_announces_and_asks_the_validate_verb() {
    let world = World::new();
    world.registry.put("shared", &pipeline_yaml("shared"));
    world.projects.add("alpha");
    world.projects.add("bravo");

    let stored = world
        .pipelines
        .validate(
            &operator(),
            "shared",
            projects(&["alpha", "bravo", "alpha"]),
        )
        .await
        .unwrap();
    assert_eq!(stored, projects(&["alpha", "bravo"]));
    assert_eq!(world.validations.of("shared"), Some(stored));

    let stored = world
        .pipelines
        .validate(&operator(), "shared", projects(&[]))
        .await
        .unwrap();
    assert_eq!(stored, PipelineValidation::None, "nothing ticked");

    world
        .pipelines
        .validate(&operator(), "shared", PipelineValidation::all())
        .await
        .unwrap();
    assert_eq!(
        world.validations.of("shared"),
        Some(PipelineValidation::all())
    );
    assert_eq!(world.events.validations_changed(), ["shared"; 3]);
    assert_eq!(
        world.host.asked()[0],
        Operation::new(ResourceKind::Pipeline, Verb::Action("validate".into())).on("shared")
    );
}

#[tokio::test]
async fn every_projet_is_stored_with_the_projets_it_is_listed_first_for() {
    let world = World::new();
    world.registry.put("shared", &pipeline_yaml("shared"));
    world.projects.add("alpha");
    world.projects.add("bravo");

    let stored = world
        .pipelines
        .validate(
            &operator(),
            "shared",
            all_listed_first_for(&["bravo", "alpha", "bravo"]),
        )
        .await
        .unwrap();
    assert_eq!(stored, all_listed_first_for(&["bravo", "alpha"]));
    assert_eq!(world.validations.of("shared"), Some(stored));

    match world
        .pipelines
        .validate(&operator(), "shared", all_listed_first_for(&["nope"]))
        .await
    {
        Err(PipelineError::UnknownProject(id)) => assert_eq!(id, "nope"),
        other => panic!("expected an unknown Projet, got {other:?}"),
    }
}

#[tokio::test]
async fn an_unknown_pipeline_or_projet_is_refused_before_any_write() {
    let world = World::new();
    world.registry.put("shared", &pipeline_yaml("shared"));
    world.projects.add("alpha");

    assert!(matches!(
        world
            .pipelines
            .validate(&operator(), "ghost", PipelineValidation::None)
            .await,
        Err(PipelineError::NotFound)
    ));
    match world
        .pipelines
        .validate(&operator(), "shared", projects(&["alpha", "nope"]))
        .await
    {
        Err(PipelineError::UnknownProject(id)) => assert_eq!(id, "nope"),
        other => panic!("expected an unknown Projet, got {other:?}"),
    }
    assert_eq!(world.validations.of("shared"), None);
    assert!(world.events.validations_changed().is_empty());
}

#[tokio::test]
async fn a_refusal_writes_nothing() {
    let world = World::refusing("read only");
    world.registry.put("shared", &pipeline_yaml("shared"));
    match world
        .pipelines
        .validate(&operator(), "shared", PipelineValidation::None)
        .await
    {
        Err(PipelineError::Denied(denied)) => assert_eq!(denied.reason, "read only"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert_eq!(world.validations.of("shared"), None);
}

#[tokio::test]
async fn a_rename_and_a_renaming_save_carry_the_validation() {
    let world = World::new();
    world.registry.put("first", &pipeline_yaml("first"));
    world.validations.set("first", projects(&["alpha"]));

    world
        .pipelines
        .rename(&operator(), "first", "Second")
        .await
        .unwrap();
    assert_eq!(world.validations.of("first"), None);
    assert_eq!(world.validations.of("second"), Some(projects(&["alpha"])));

    world
        .pipelines
        .save(
            &operator(),
            "second",
            &pipeline_yaml("Third"),
            &HashMap::new(),
        )
        .await
        .unwrap();
    assert_eq!(world.validations.of("second"), None);
    assert_eq!(world.validations.of("third"), Some(projects(&["alpha"])));
}

#[tokio::test]
async fn a_deletion_forgets_the_validation() {
    let world = World::new();
    world.registry.put("gone", &pipeline_yaml("gone"));
    world.validations.set("gone", PipelineValidation::None);
    world.pipelines.delete(&operator(), "gone").await.unwrap();
    assert_eq!(world.validations.of("gone"), None);
}

#[tokio::test]
async fn a_deleted_projet_is_withdrawn_and_all_stays_all() {
    let world = World::new();
    world.validations.set("both", projects(&["alpha", "bravo"]));
    world.validations.set("alone", projects(&["alpha"]));
    world.validations.set("other", projects(&["bravo"]));
    world.validations.set("global", PipelineValidation::all());
    world
        .validations
        .set("first", all_listed_first_for(&["alpha", "bravo"]));
    world
        .validations
        .set("first-alone", all_listed_first_for(&["alpha"]));

    world.pipelines.forget_project("alpha").await;

    assert_eq!(world.validations.of("both"), Some(projects(&["bravo"])));
    assert_eq!(
        world.validations.of("alone"),
        Some(PipelineValidation::None)
    );
    assert_eq!(world.validations.of("other"), Some(projects(&["bravo"])));
    assert_eq!(
        world.validations.of("global"),
        Some(PipelineValidation::all())
    );
    assert_eq!(
        world.validations.of("first"),
        Some(all_listed_first_for(&["bravo"]))
    );
    assert_eq!(
        world.validations.of("first-alone"),
        Some(PipelineValidation::all()),
        "every Projet stays every Projet"
    );
    assert_eq!(
        world.events.validations_changed(),
        ["alone", "both", "first", "first-alone"]
    );
}
