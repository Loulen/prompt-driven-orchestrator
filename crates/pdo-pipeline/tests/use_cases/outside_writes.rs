//! What changes a shared draft from outside the canvas (ADR-0088): a file
//! written outside PDO, kept as a version « external edit » and merged in,
//! and the library assistant, which saves into the draft as a participant of
//! its Actor and never writes the file.

use std::collections::HashMap;

use pdo_pipeline::adapters::draft_doc::YDraftDocuments;
use pdo_pipeline::application::drafts::{DraftJoin, DraftMessage};
use pdo_pipeline::application::ports::DraftDocuments;
use pdo_pipeline::domain::draft::notice::DraftNotice;
use pdo_pipeline::domain::draft::DraftContent;
use serde_json::json;

use crate::fakes::*;

fn world_with_worker() -> World {
    let world = World::new();
    world.registry.put("flow", &pipeline_with_worker("flow"));
    world.registry.put_prompt("flow", "worker", "Do the work.");
    world
}

async fn join(world: &World) -> DraftJoin {
    world
        .pipelines
        .join_draft(&operator(), "flow")
        .await
        .unwrap()
}

/// Change the draft as the participant `join` would, from its own copy.
async fn edit(world: &World, join: &DraftJoin, edit: impl FnOnce(&mut DraftContent)) {
    let copy = YDraftDocuments.load(&join.room.encode()).unwrap();
    let before = copy.content();
    let mut after = before.clone();
    edit(&mut after);
    let update = copy.change(&before, &after);
    world
        .pipelines
        .edit_draft(&join.room, join.connection, &update)
        .await
        .unwrap();
}

fn content(join: &DraftJoin) -> DraftContent {
    YDraftDocuments.load(&join.room.encode()).unwrap().content()
}

fn worker(content: &DraftContent) -> serde_json::Value {
    content.pipeline["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == "worker")
        .unwrap()
        .clone()
}

fn notices(join: &mut DraftJoin) -> Vec<DraftNotice> {
    let mut heard = Vec::new();
    while let Ok(message) = join.messages.try_recv() {
        if let DraftMessage::Notice(notice) = message {
            heard.push(notice);
        }
    }
    heard
}

#[tokio::test]
async fn a_file_written_outside_is_a_version_merged_into_the_draft_with_a_notice() {
    let world = world_with_worker();
    world.pipelines.versions("flow").await.unwrap();
    let mut alice = join(&world).await;
    edit(&world, &alice, |c| {
        c.pipeline["nodes"][1]["view"] = json!({"x": 10, "y": 20});
    })
    .await;
    world.registry.put(
        "flow",
        &pipeline_with_worker("flow").replace("name: Worker", "name: Reviewer"),
    );
    world.registry.put_prompt("flow", "worker", "Review it.");

    world
        .pipelines
        .file_written_outside("flow", "/registry/flow.yaml")
        .await;

    assert_eq!(
        world.versions.titles("flow"),
        vec![(1, "1.0".into()), (2, "external edit".into())]
    );
    let kept = world.versions.of("flow", 2).unwrap();
    assert!(kept.meta.external);
    assert_eq!(kept.meta.author, None);
    assert!(kept.yaml.contains("name: Reviewer"));
    assert!(world.events.3.lock().unwrap().contains(&"flow".to_string()));
    // Both changes are there: the draft's move, the file's name and prompt.
    let merged = content(&alice);
    assert_eq!(worker(&merged)["name"], "Reviewer");
    assert_eq!(worker(&merged)["view"], json!({"x": 10, "y": 20}));
    assert_eq!(merged.prompts["worker"], "Review it.");
    assert_eq!(alice.room.changes(), 1);
    let DraftNotice::ExternalEdit {
        file,
        version,
        fields,
    } = notices(&mut alice).pop().unwrap()
    else {
        panic!("an external edit is announced");
    };
    assert_eq!(file, "flow.yaml");
    assert_eq!(
        (version.number, version.title.as_str()),
        (2, "external edit")
    );
    assert_eq!(fields, vec!["name of worker", "prompt of worker"]);
}

#[tokio::test]
async fn the_same_outside_write_heard_twice_is_one_version() {
    let world = world_with_worker();
    world.pipelines.versions("flow").await.unwrap();
    world.registry.put_prompt("flow", "worker", "Review it.");
    world
        .pipelines
        .file_written_outside("flow", "/r/flow.yaml")
        .await;
    world
        .pipelines
        .file_written_outside("flow", "/r/flow.prompts/worker.md")
        .await;
    assert_eq!(world.versions.titles("flow").len(), 2);
}

#[tokio::test]
async fn a_pipeline_with_no_version_yet_becomes_v1_as_written() {
    let world = world_with_worker();
    world
        .pipelines
        .file_written_outside("flow", "/r/flow.yaml")
        .await;
    assert_eq!(world.versions.titles("flow"), vec![(1, "1.0".into())]);
}

#[tokio::test]
async fn the_assistant_saves_into_the_draft_never_the_file() {
    let world = world_with_worker();
    let mut alice = join(&world).await;
    // Alice renamed the end node meanwhile, unsaved.
    edit(&world, &alice, |c| {
        c.pipeline["nodes"][2]["name"] = json!("Finish");
    })
    .await;
    let file = world.registry.get("flow").unwrap();

    let yaml = pipeline_with_worker("flow").replace("name: Worker", "name: Builder");
    let prompts = HashMap::from([("worker".to_string(), "Build it.".to_string())]);
    let fields = world
        .pipelines
        .assistant_save(&operator(), "flow", &yaml, &prompts)
        .await
        .unwrap()
        .fields;

    assert_eq!(fields, vec!["name of worker", "prompt of worker"]);
    let after = world.registry.get("flow").unwrap();
    assert_eq!(
        (after.yaml, after.prompts),
        (file.yaml, file.prompts),
        "no file is written"
    );
    let draft = content(&alice);
    assert_eq!(worker(&draft)["name"], "Builder");
    assert_eq!(draft.prompts["worker"], "Build it.");
    assert_eq!(
        draft.pipeline["nodes"][2]["name"], "Finish",
        "Alice's change stays"
    );
    // The assistant sits in the draft, as a participant of its Actor, on the
    // node it changed.
    let participants = alice.room.summary().participants;
    let assistant = participants.iter().find(|p| p.assistant).unwrap();
    assert_eq!(assistant.actor, operator());
    assert_eq!(assistant.presence.selection.as_ref().unwrap().id, "worker");
    let heard = notices(&mut alice);
    assert!(matches!(
        heard.last(),
        Some(DraftNotice::Assistant { fields, .. }) if fields.len() == 2
    ));
}

#[tokio::test]
async fn the_assistant_reads_the_draft_and_its_next_save_keeps_its_last() {
    let world = world_with_worker();
    let alice = join(&world).await;
    edit(&world, &alice, |c| {
        c.pipeline["nodes"][2]["name"] = json!("Finish");
    })
    .await;
    let read = world
        .pipelines
        .assistant_read(&operator(), "flow")
        .await
        .unwrap();
    assert!(
        read.yaml.contains("Finish"),
        "it reads the draft: {}",
        read.yaml
    );
    assert_eq!(read.prompts["worker"], "Do the work.");

    // First save: the worker renamed.
    let first = read.yaml.replace("Worker", "Builder");
    world
        .pipelines
        .assistant_save(&operator(), "flow", &first, &HashMap::new())
        .await
        .unwrap();
    // Alice changes the start node meanwhile.
    edit(&world, &alice, |c| {
        c.pipeline["nodes"][0]["name"] = json!("Begin");
    })
    .await;
    // Second save, from its own last one: a prompt only.
    let prompts = HashMap::from([("worker".to_string(), "Build it.".to_string())]);
    let fields = world
        .pipelines
        .assistant_save(&operator(), "flow", &first, &prompts)
        .await
        .unwrap()
        .fields;
    assert_eq!(fields, vec!["prompt of worker"]);

    let draft = content(&alice);
    assert_eq!(worker(&draft)["name"], "Builder");
    assert_eq!(draft.prompts["worker"], "Build it.");
    assert_eq!(draft.pipeline["nodes"][0]["name"], "Begin");
    assert_eq!(draft.pipeline["nodes"][2]["name"], "Finish");
}

#[tokio::test]
async fn the_assistant_leaves_with_its_focus_and_does_not_hold_a_draft_open() {
    let world = world_with_worker();
    let alice = join(&world).await;
    let yaml = pipeline_with_worker("flow").replace("name: Worker", "name: Builder");
    world
        .pipelines
        .assistant_save(&operator(), "flow", &yaml, &HashMap::new())
        .await
        .unwrap();
    assert_eq!(alice.room.summary().participants.len(), 2);

    // Leaving another pipeline keeps its seat here.
    world.pipelines.assistant_left(&operator(), "other").await;
    assert_eq!(alice.room.summary().participants.len(), 2);
    world.pipelines.assistant_left(&operator(), "flow").await;
    assert_eq!(alice.room.summary().participants.len(), 1);

    world
        .pipelines
        .assistant_save(
            &operator(),
            "flow",
            &yaml.replace("Builder", "Maker"),
            &HashMap::new(),
        )
        .await
        .unwrap();
    world
        .pipelines
        .leave_draft(&alice.room, alice.connection)
        .await;
    assert!(world.pipelines.draft_summaries().await["flow"]
        .participants
        .is_empty());
}

#[tokio::test]
async fn an_assistant_save_that_does_not_parse_changes_nothing() {
    let world = world_with_worker();
    let alice = join(&world).await;
    let refused = world
        .pipelines
        .assistant_save(&operator(), "flow", "nodes: [unclosed", &HashMap::new())
        .await;
    assert!(refused.is_err());
    assert_eq!(alice.room.changes(), 0);
    assert_eq!(alice.room.summary().participants.len(), 1);
}

#[tokio::test]
async fn a_file_that_does_not_read_has_no_draft_and_the_assistant_saves_a_version() {
    let world = World::new();
    world.registry.put("flow", "name: flow\nnodes: []\n");
    let read = world
        .pipelines
        .assistant_read(&operator(), "flow")
        .await
        .unwrap();
    assert_eq!(read.yaml, "name: flow\nnodes: []\n", "the file itself");

    let saved = world
        .pipelines
        .assistant_save(
            &operator(),
            "flow",
            &pipeline_with_worker("flow"),
            &HashMap::new(),
        )
        .await
        .unwrap();
    assert!(saved.fields.is_empty());
    assert_eq!(saved.version.unwrap().number, 2);
    assert_eq!(
        world.registry.get("flow").unwrap().yaml,
        pipeline_with_worker("flow")
    );
    let kept = world.versions.of("flow", 2).unwrap();
    assert_eq!(kept.meta.author, Some(operator()));
}

#[tokio::test]
async fn an_outside_write_that_does_not_read_is_no_version() {
    let world = world_with_worker();
    world.pipelines.versions("flow").await.unwrap();
    world.registry.put("flow", "name: flow\nnodes: [unclosed");
    world
        .pipelines
        .file_written_outside("flow", "/r/flow.yaml")
        .await;
    assert_eq!(world.versions.titles("flow").len(), 1);
    assert!(world.events.3.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_draft_only_the_assistant_sat_in_closes_with_nobody_listed() {
    let world = world_with_worker();
    let yaml = pipeline_with_worker("flow").replace("name: Worker", "name: Builder");
    world
        .pipelines
        .assistant_save(&operator(), "flow", &yaml, &HashMap::new())
        .await
        .unwrap();
    let announced = world.events.last_draft("flow").unwrap();
    assert!(announced.participants.is_empty(), "{announced:?}");
    assert_eq!(announced.changes, 1);
}
