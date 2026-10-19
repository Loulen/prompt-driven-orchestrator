use super::*;
use pdo_harness::registry::CLAUDE;

fn message(uuid: &str, role: &str, text: &str) -> String {
    serde_json::json!({
        "type": role, "uuid": uuid, "timestamp": "2026-10-19T10:00:00Z",
        "message": {"role": role, "content": [{"type": "text", "text": text}]}
    })
    .to_string()
        + "\n"
}

fn target(dir: &std::path::Path, node_id: &str, iter: i64, lines: &[String]) -> SearchTarget {
    let path = dir.join(format!("{node_id}-{iter}.jsonl"));
    std::fs::write(&path, lines.concat()).unwrap();
    SearchTarget {
        node_id: node_id.to_string(),
        iter,
        harness: CLAUDE.to_string(),
        path,
    }
}

fn never() -> bool {
    false
}

fn shape(answer: &RunTranscriptSearch) -> Vec<String> {
    answer
        .hits
        .iter()
        .map(|h| {
            let index = match &h.hit {
                TranscriptHit::Message { index, .. } | TranscriptHit::Tool { index, .. } => index,
            };
            format!("{}#{}:{index}", h.node_id, h.iter)
        })
        .collect()
}

#[test]
fn the_order_is_the_declared_nodes_then_the_others_each_by_iteration() {
    let iterations = HashMap::from([
        ("reviewer".to_string(), vec![1]),
        ("builder".to_string(), vec![2, 1]),
        ("zeta".to_string(), vec![1]),
        ("alpha".to_string(), vec![3]),
    ]);
    assert_eq!(
        search_order(&["builder", "designer", "reviewer"], &iterations),
        vec![
            ("builder".to_string(), 1),
            ("builder".to_string(), 2),
            ("reviewer".to_string(), 1),
            ("alpha".to_string(), 3),
            ("zeta".to_string(), 1),
        ]
    );
}

#[test]
fn a_run_search_lists_every_node_iteration_with_a_match_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let targets = vec![
        target(
            dir.path(),
            "builder",
            1,
            &[
                message("u1", "user", "Add a debounce."),
                message("a1", "assistant", "Nothing to see."),
                message("a2", "assistant", "The DEBOUNCE is in."),
            ],
        ),
        target(
            dir.path(),
            "builder",
            2,
            &[message("u1", "user", "Debounce again.")],
        ),
        target(
            dir.path(),
            "designer",
            1,
            &[message("u1", "user", "No match here.")],
        ),
        target(
            dir.path(),
            "reviewer",
            1,
            &[message("a1", "assistant", "the debounce lacks a test")],
        ),
    ];
    let reader = TranscriptReader::new(8);
    let answer = search_targets(
        &reader,
        &targets,
        &Needle::new("debounce").unwrap(),
        10,
        &never,
    )
    .unwrap();
    assert_eq!(
        shape(&answer),
        vec!["builder#1:0", "builder#1:2", "builder#2:0", "reviewer#1:0"]
    );
    assert_eq!(answer.matched, 4);
    assert_eq!(
        answer.nodes,
        vec![
            NodeTranscriptMatches {
                node_id: "builder".into(),
                matched: 3
            },
            NodeTranscriptMatches {
                node_id: "reviewer".into(),
                matched: 1
            },
        ]
    );
    assert!(!answer.truncated && !answer.cancelled);
}

#[test]
fn the_hits_are_bounded_across_files_and_every_match_counted() {
    let dir = tempfile::tempdir().unwrap();
    let lines: Vec<String> = (0..5)
        .map(|i| message(&format!("u{i}"), "user", "debounce"))
        .collect();
    let targets = vec![
        target(dir.path(), "a", 1, &lines),
        target(dir.path(), "b", 1, &lines),
    ];
    let reader = TranscriptReader::new(8);
    let answer = search_targets(
        &reader,
        &targets,
        &Needle::new("debounce").unwrap(),
        7,
        &never,
    )
    .unwrap();
    assert_eq!(answer.hits.len(), 7);
    assert_eq!(answer.matched, 10);
    assert!(answer.truncated);
    assert_eq!(
        answer.nodes.iter().map(|n| n.matched).collect::<Vec<_>>(),
        vec![5, 5]
    );
}

#[test]
fn a_file_gone_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let mut gone = target(dir.path(), "a", 1, &[]);
    gone.path = dir.path().join("never-written.jsonl");
    let targets = vec![
        gone,
        target(dir.path(), "b", 1, &[message("u", "user", "debounce")]),
    ];
    let answer = search_targets(
        &TranscriptReader::new(8),
        &targets,
        &Needle::new("debounce").unwrap(),
        10,
        &never,
    )
    .unwrap();
    assert_eq!(shape(&answer), vec!["b#1:0"]);
}

#[test]
fn a_cancelled_search_stops_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let targets = vec![
        target(dir.path(), "a", 1, &[message("u", "user", "debounce")]),
        target(dir.path(), "b", 1, &[message("u", "user", "debounce")]),
    ];
    let answer = search_targets(
        &TranscriptReader::new(8),
        &targets,
        &Needle::new("debounce").unwrap(),
        10,
        &|| true,
    )
    .unwrap();
    assert!(answer.cancelled);
    assert!(answer.hits.is_empty());
}

#[test]
fn a_new_search_of_the_same_client_cancels_the_previous_one() {
    let cancels = SearchCancels::default();
    let first = cancels.begin(Some("box-1"));
    let other = cancels.begin(Some("box-2"));
    let second = cancels.begin(Some("box-1"));
    assert!(first.load(Ordering::Relaxed), "the next keystroke cancels");
    assert!(
        !other.load(Ordering::Relaxed),
        "another client's search runs on"
    );
    assert!(!second.load(Ordering::Relaxed));

    // The first one ending late does not forget the second.
    cancels.finish(Some("box-1"), &first);
    assert_eq!(cancels.in_flight(), 2);
    cancels.finish(Some("box-1"), &second);
    cancels.finish(Some("box-2"), &other);
    assert_eq!(cancels.in_flight(), 0);

    // Without a client, nothing is registered.
    let anonymous = cancels.begin(None);
    assert_eq!(cancels.in_flight(), 0);
    cancels.finish(None, &anonymous);
}
