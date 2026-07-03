//! Layer 3a — canvas-note round-trip (#307 / ADR-0018): a top-level `notes:`
//! block survives load → PUT (identity save) → GET, in BOTH the raw `yaml`
//! string AND the daemon-parsed `pipeline` form. The parsed form is the decisive
//! assertion: the frontend rehydrates from `pipeline`, never the raw YAML — a
//! missing Rust field would let the note live on disk yet vanish from the UI on
//! reload (the D2 failure mode).

mod common;

use common::TestDaemon;

const NOTE_MARKER: &str = "NOTE-307 — bounded loop volontairement limité à 3";

fn pipeline_with_note_yaml() -> String {
    format!(
        r#"name: notes-roundtrip
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: {{ node: start, port: user_prompt }}
    target: {{ node: end, port: result }}
notes:
  - id: note-abc
    content: "{NOTE_MARKER}"
    view:
      x: 128.0
      y: 256.0
"#
    )
}

#[tokio::test]
async fn note_round_trips_through_save_in_yaml_and_parsed_form() {
    let daemon = TestDaemon::spawn(|repo| {
        let dir = repo.join(".pdo").join("pipelines");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("notes-roundtrip.yaml"), pipeline_with_note_yaml())?;
        Ok(())
    })
    .await
    .unwrap();

    let client = reqwest::Client::new();

    // GET — the note must be present in both the raw yaml and the parsed form
    // right out of the loader (parse-time), before any save round-trip.
    let resp = client
        .get(format!("{}/pipelines/notes-roundtrip", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let detail: serde_json::Value = resp.json().await.unwrap();

    let yaml = detail["yaml"].as_str().unwrap();
    assert!(
        yaml.contains("notes:") && yaml.contains(NOTE_MARKER),
        "raw yaml should carry the top-level notes block; got:\n{yaml}"
    );

    let notes = detail["pipeline"]["notes"]
        .as_array()
        .expect("parsed pipeline must expose a `notes` array (missing Rust field?)");
    assert_eq!(notes.len(), 1, "expected exactly one parsed note");
    assert_eq!(notes[0]["id"], "note-abc");
    assert_eq!(notes[0]["content"], NOTE_MARKER);
    assert_eq!(notes[0]["view"]["x"], 128.0);
    assert_eq!(notes[0]["view"]["y"], 256.0);

    // PUT (identity save)
    let body = serde_json::json!({ "yaml": yaml, "prompts": {} });
    let resp = client
        .put(format!("{}/pipelines/notes-roundtrip", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "PUT rejected its own YAML: {:?}",
        resp.text().await
    );

    // GET again — the note must survive the save round-trip in both forms.
    let resp = client
        .get(format!("{}/pipelines/notes-roundtrip", daemon.url()))
        .send()
        .await
        .unwrap();
    let detail2: serde_json::Value = resp.json().await.unwrap();

    let yaml2 = detail2["yaml"].as_str().unwrap();
    assert!(
        yaml2.contains("notes:") && yaml2.contains(NOTE_MARKER),
        "note dropped from yaml after round-trip; got:\n{yaml2}"
    );

    let notes2 = detail2["pipeline"]["notes"]
        .as_array()
        .expect("parsed pipeline must still expose `notes` after round-trip");
    assert_eq!(notes2.len(), 1, "note count changed after round-trip");
    assert_eq!(notes2[0]["content"], NOTE_MARKER);
    assert_eq!(notes2[0]["view"]["x"], 128.0);

    // The note must never leak into the DAG: it is not a node.
    let node_ids: Vec<&str> = detail2["pipeline"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["id"].as_str().unwrap())
        .collect();
    assert!(
        !node_ids.contains(&"note-abc"),
        "a note must never appear in pipeline.nodes; got {node_ids:?}"
    );
}
