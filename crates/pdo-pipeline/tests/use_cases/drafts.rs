//! The shared draft of a pipeline (ADR-0088): joined, changed by its
//! participants, kept across a restart, following the file written from
//! elsewhere.

use std::collections::HashMap;

use pdo_pipeline::adapters::draft_doc::YDraftDocuments;
use pdo_pipeline::application::drafts::{DraftJoin, DraftMessage};
use pdo_pipeline::application::ports::{DraftDocument, DraftDocuments};
use pdo_pipeline::domain::draft::DraftContent;
use pdo_pipeline::domain::version::SaveAs;
use serde_json::json;

use crate::fakes::*;

/// A participant's own copy of the draft, as a browser holds it.
struct Client {
    doc: Box<dyn DraftDocument>,
}

impl Client {
    fn of(join: &DraftJoin) -> Self {
        Self {
            doc: YDraftDocuments.load(&join.room.encode()).unwrap(),
        }
    }

    /// Change the copy with `edit`; the update to send.
    fn edit(&self, edit: impl FnOnce(&mut DraftContent)) -> Vec<u8> {
        let before = self.doc.content();
        let mut after = before.clone();
        edit(&mut after);
        self.doc.change(&before, &after)
    }
}

fn world_with_worker() -> World {
    let world = World::new();
    world.registry.put("flow", &pipeline_with_worker("flow"));
    world.registry.put_prompt("flow", "worker", "Do the work.");
    world
}

fn node<'a>(content: &'a DraftContent, id: &str) -> &'a serde_json::Value {
    content.pipeline["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == id)
        .unwrap()
}

#[tokio::test]
async fn the_first_participant_opens_the_draft_from_the_file() {
    let world = world_with_worker();
    let join = world
        .pipelines
        .join_draft(&operator(), "flow")
        .await
        .unwrap();
    assert!(join.can_write);
    let content = Client::of(&join).doc.content();
    assert_eq!(node(&content, "worker")["name"], "Worker");
    assert_eq!(content.prompts["worker"], "Do the work.");
    let announced = world.events.last_draft("flow").unwrap();
    assert_eq!(announced.changes, 0);
    assert_eq!(announced.participants.len(), 1);
    assert!(world.drafts.0.lock().unwrap().contains_key("flow"));
}

#[tokio::test]
async fn a_change_reaches_the_other_participants_and_is_counted() {
    let world = world_with_worker();
    let alice = world
        .pipelines
        .join_draft(&operator(), "flow")
        .await
        .unwrap();
    let mut bob = world
        .pipelines
        .join_draft(&operator(), "flow")
        .await
        .unwrap();
    let update = Client::of(&alice).edit(|c| {
        c.pipeline["nodes"][1]["name"] = json!("Builder");
        c.prompts.insert("worker".into(), "Build it.".into());
    });
    world
        .pipelines
        .edit_draft(&alice.room, alice.connection, &update)
        .await
        .unwrap();
    let mut heard_update = false;
    while let Ok(message) = bob.messages.try_recv() {
        if let DraftMessage::Update { from, .. } = message {
            assert_eq!(from, alice.connection);
            heard_update = true;
        }
    }
    assert!(heard_update);
    assert_eq!(alice.room.changes(), 1);
    assert_eq!(world.events.last_draft("flow").unwrap().changes, 1);
}

#[tokio::test]
async fn the_draft_survives_a_restart() {
    let world = world_with_worker();
    let join = world
        .pipelines
        .join_draft(&operator(), "flow")
        .await
        .unwrap();
    let update = Client::of(&join).edit(|c| {
        c.pipeline["nodes"][1]["view"] = json!({"x": 420, "y": 80});
    });
    world
        .pipelines
        .edit_draft(&join.room, join.connection, &update)
        .await
        .unwrap();
    let generation = join.room.generation();

    let restarted = world.restarted();
    let again = restarted
        .pipelines
        .join_draft(&operator(), "flow")
        .await
        .unwrap();
    assert_eq!(again.room.generation(), generation);
    let content = Client::of(&again).doc.content();
    assert_eq!(node(&content, "worker")["view"], json!({"x": 420, "y": 80}));
    assert_eq!(again.room.changes(), 1);
}

#[tokio::test]
async fn a_kept_draft_is_counted_again_when_it_opens() {
    let world = world_with_worker();
    let join = world
        .pipelines
        .join_draft(&operator(), "flow")
        .await
        .unwrap();
    world
        .pipelines
        .leave_draft(&join.room, join.connection)
        .await;
    {
        let mut kept = world.drafts.0.lock().unwrap();
        let flow = kept.get_mut("flow").unwrap();
        (flow.changes, flow.nodes) = (4, 0);
    }
    let again = world
        .pipelines
        .join_draft(&operator(), "flow")
        .await
        .unwrap();
    assert_eq!((again.room.changes(), again.room.summary().nodes), (0, 3));
}

#[tokio::test]
async fn a_participant_who_may_not_update_joins_to_read() {
    let world = world_with_worker();
    let reader = world
        .refusing_over("read only")
        .pipelines
        .join_draft(&operator(), "flow")
        .await
        .unwrap();
    assert!(!reader.can_write);
    assert!(!reader.room.summary().participants[0].can_write);
}

#[tokio::test]
async fn a_file_written_elsewhere_lands_in_the_draft_beside_its_changes() {
    let world = world_with_worker();
    let join = world
        .pipelines
        .join_draft(&operator(), "flow")
        .await
        .unwrap();
    // In the draft: the worker moved.
    let update = Client::of(&join).edit(|c| {
        c.pipeline["nodes"][1]["view"] = json!({"x": 10, "y": 20});
    });
    world
        .pipelines
        .edit_draft(&join.room, join.connection, &update)
        .await
        .unwrap();
    // In the file: the worker renamed, and a new prompt.
    world.registry.put(
        "flow",
        &pipeline_with_worker("flow").replace("name: Worker", "name: Reviewer"),
    );
    world.registry.put_prompt("flow", "worker", "Review it.");
    world.pipelines.draft_file_changed("flow").await;

    let content = Client::of(&join).doc.content();
    assert_eq!(node(&content, "worker")["name"], "Reviewer");
    assert_eq!(node(&content, "worker")["view"], json!({"x": 10, "y": 20}));
    assert_eq!(content.prompts["worker"], "Review it.");
    // Only the move is left to save.
    assert_eq!(join.room.changes(), 1);
}

#[tokio::test]
async fn a_draft_nobody_opened_ignores_the_file() {
    let world = world_with_worker();
    world.pipelines.draft_file_changed("flow").await;
    assert!(world.drafts.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn saving_the_draft_leaves_no_change() {
    let world = world_with_worker();
    let join = world
        .pipelines
        .join_draft(&operator(), "flow")
        .await
        .unwrap();
    let client = Client::of(&join);
    let update = client.edit(|c| {
        c.pipeline["nodes"][1]["name"] = json!("Builder");
    });
    world
        .pipelines
        .edit_draft(&join.room, join.connection, &update)
        .await
        .unwrap();
    let yaml = pipeline_with_worker("flow").replace("name: Worker", "name: Builder");
    world
        .pipelines
        .save_draft(
            &operator(),
            "flow",
            &yaml,
            &HashMap::new(),
            &client.doc.content(),
            &SaveAs::new_version(),
        )
        .await
        .unwrap();
    assert_eq!(join.room.changes(), 0);
    assert_eq!(world.drafts.0.lock().unwrap()["flow"].changes, 0);
}

/// The file's coordinates read back as floats (`470.0`), the draft's as the
/// editor and the replicated document hold them (`470`): the same file.
#[tokio::test]
async fn a_file_with_coordinates_opens_and_saves_with_no_change() {
    let placed = |yaml: String| {
        yaml.replace(
            "    type: agent\n",
            "    type: agent\n    view: { x: 470, y: 120 }\n",
        )
    };
    let world = World::new();
    world
        .registry
        .put("flow", &placed(pipeline_with_worker("flow")));
    let join = world
        .pipelines
        .join_draft(&operator(), "flow")
        .await
        .unwrap();
    assert_eq!(join.room.changes(), 0);
    assert!(
        world
            .pipelines
            .draft_summaries()
            .await
            .get("flow")
            .unwrap()
            .changes
            == 0
    );

    let client = Client::of(&join);
    let update = client.edit(|c| {
        c.pipeline["nodes"][1]["name"] = json!("Builder");
    });
    world
        .pipelines
        .edit_draft(&join.room, join.connection, &update)
        .await
        .unwrap();
    assert_eq!(join.room.changes(), 1);
    let yaml = placed(pipeline_with_worker("flow")).replace("name: Worker", "name: Builder");
    world
        .pipelines
        .save_draft(
            &operator(),
            "flow",
            &yaml,
            &HashMap::new(),
            &client.doc.content(),
            &SaveAs::new_version(),
        )
        .await
        .unwrap();
    assert_eq!(join.room.changes(), 0);

    // Opened again from the store, after its last participant left.
    world
        .pipelines
        .leave_draft(&join.room, join.connection)
        .await;
    let again = world
        .pipelines
        .join_draft(&operator(), "flow")
        .await
        .unwrap();
    assert_eq!(again.room.changes(), 0);
}

/// A drawn edge lands at a sub-pixel offset (`13.270833333333343`), which the
/// save rounds in the file. The draft the save carries arrives as JSON text:
/// read back to the very float the replicated document holds, the edge is
/// still found by its value, takes the file's rounded offset, and nothing is
/// left to save (the « v2 · draft: 3 changes » of the First pipeline tour).
#[tokio::test]
async fn an_edge_at_a_sub_pixel_offset_saves_with_no_change() {
    let world = world_with_worker();
    let join = world
        .pipelines
        .join_draft(&operator(), "flow")
        .await
        .unwrap();
    let client = Client::of(&join);
    let update = client.edit(|c| {
        c.pipeline["edges"][1]["source_anchor"] =
            json!({"side": "right", "offset": 13.270833333333343_f64});
    });
    world
        .pipelines
        .edit_draft(&join.room, join.connection, &update)
        .await
        .unwrap();
    let yaml = pipeline_with_worker("flow").replace(
        "    target: { node: end, port: result }\n",
        "    target: { node: end, port: result }\n    source_anchor: { side: right, offset: 13 }\n",
    );
    // What `PUT /pipelines/{id}` reads of the body the editor sent.
    let sent: DraftContent =
        serde_json::from_str(&serde_json::to_string(&client.doc.content()).unwrap()).unwrap();
    world
        .pipelines
        .save_draft(
            &operator(),
            "flow",
            &yaml,
            &HashMap::new(),
            &sent,
            &SaveAs::new_version(),
        )
        .await
        .unwrap();
    assert_eq!(join.room.changes(), 0);
    let content = Client::of(&join).doc.content();
    assert_eq!(content.pipeline["edges"][1]["source_anchor"]["offset"], 13);
}

#[tokio::test]
async fn a_change_made_while_saving_is_still_a_change_and_survives_a_restart() {
    let world = world_with_worker();
    let alice = world
        .pipelines
        .join_draft(&operator(), "flow")
        .await
        .unwrap();
    let bob = world
        .pipelines
        .join_draft(&operator(), "flow")
        .await
        .unwrap();
    // Alice serializes the draft for her save...
    let saved = Client::of(&alice).doc.content();
    // ...while Bob moves the worker.
    let update = Client::of(&bob).edit(|c| {
        c.pipeline["nodes"][1]["view"] = json!({"x": 7, "y": 8});
    });
    world
        .pipelines
        .edit_draft(&bob.room, bob.connection, &update)
        .await
        .unwrap();
    world
        .pipelines
        .save_draft(
            &operator(),
            "flow",
            &pipeline_with_worker("flow"),
            &HashMap::new(),
            &saved,
            &SaveAs::new_version(),
        )
        .await
        .unwrap();
    assert_eq!(alice.room.changes(), 1);

    let restarted = world.restarted();
    let again = restarted
        .pipelines
        .join_draft(&operator(), "flow")
        .await
        .unwrap();
    let content = Client::of(&again).doc.content();
    assert_eq!(node(&content, "worker")["view"], json!({"x": 7, "y": 8}));
    assert_eq!(again.room.changes(), 1);
}

#[tokio::test]
async fn a_rename_moves_the_draft_and_tells_its_participants() {
    let world = world_with_worker();
    let mut join = world
        .pipelines
        .join_draft(&operator(), "flow")
        .await
        .unwrap();
    world
        .pipelines
        .rename(&operator(), "flow", "Shipping")
        .await
        .unwrap();
    assert_eq!(join.room.id(), "shipping");
    assert!(world.drafts.0.lock().unwrap().contains_key("shipping"));
    let mut moved = None;
    while let Ok(message) = join.messages.try_recv() {
        if let DraftMessage::Moved { to } = message {
            moved = Some(to);
        }
    }
    assert_eq!(moved.as_deref(), Some("shipping"));
    assert_eq!(Client::of(&join).doc.content().pipeline["name"], "Shipping");
}

#[tokio::test]
async fn deleting_the_pipeline_deletes_its_draft() {
    let world = world_with_worker();
    let mut join = world
        .pipelines
        .join_draft(&operator(), "flow")
        .await
        .unwrap();
    world.pipelines.delete(&operator(), "flow").await.unwrap();
    assert!(world.drafts.0.lock().unwrap().is_empty());
    let mut removed = false;
    while let Ok(message) = join.messages.try_recv() {
        removed |= matches!(message, DraftMessage::Removed);
    }
    assert!(removed);
    // Its last participant leaving keeps nothing of it.
    world
        .pipelines
        .leave_draft(&join.room, join.connection)
        .await;
    assert!(world.drafts.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn the_last_participant_to_leave_closes_the_draft_and_the_store_keeps_it() {
    let world = world_with_worker();
    let join = world
        .pipelines
        .join_draft(&operator(), "flow")
        .await
        .unwrap();
    world
        .pipelines
        .leave_draft(&join.room, join.connection)
        .await;
    assert_eq!(
        world.events.last_draft("flow").unwrap().participants.len(),
        0
    );
    assert!(world.pipelines.draft_summaries().await.is_empty());
    assert!(world.drafts.0.lock().unwrap().contains_key("flow"));
}

#[tokio::test]
async fn the_list_shows_the_open_drafts_and_the_kept_changes() {
    let world = world_with_worker();
    let join = world
        .pipelines
        .join_draft(&operator(), "flow")
        .await
        .unwrap();
    let update = Client::of(&join).edit(|c| {
        c.pipeline["nodes"][1]["name"] = json!("Builder");
        c.pipeline["nodes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": "extra", "name": "Extra", "type": "agent"}));
    });
    world
        .pipelines
        .edit_draft(&join.room, join.connection, &update)
        .await
        .unwrap();
    let open = world.pipelines.draft_summaries().await;
    assert_eq!(open["flow"].participants.len(), 1);
    assert_eq!(open["flow"].nodes, 4);
    world
        .pipelines
        .leave_draft(&join.room, join.connection)
        .await;
    let kept = world.pipelines.draft_summaries().await;
    assert_eq!(kept["flow"].changes, 2);
    assert_eq!(kept["flow"].nodes, 4);
    assert!(kept["flow"].participants.is_empty());
}
