use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::event_log::{self, EventKind, NodeStatus};
use crate::outputs_validator;
use crate::pipeline;

pub const STALE_THRESHOLD: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detection {
    SessionDied,
    AutoComplete,
    Stale,
    Ok,
}

/// Inputs gathered by the caller (side-effectful layer) and passed into
/// the pure decision function.
pub struct NodeProbe {
    pub session_alive: bool,
    pub jsonl_mtime: Option<SystemTime>,
    pub now: SystemTime,
    pub artifacts_valid: Option<bool>,
}

/// Pure decision logic: given the probe results, determine the detection.
pub fn decide(probe: &NodeProbe) -> Detection {
    if !probe.session_alive {
        return Detection::SessionDied;
    }

    let Some(mtime) = probe.jsonl_mtime else {
        return Detection::Ok;
    };

    let age = probe.now.duration_since(mtime).unwrap_or(Duration::ZERO);
    if age < STALE_THRESHOLD {
        return Detection::Ok;
    }

    match probe.artifacts_valid {
        Some(true) => Detection::AutoComplete,
        _ => Detection::Stale,
    }
}

/// Encode a working directory path the same way Claude Code does for its
/// projects directory.
///
/// Example: `/home/user/project` → `home-user-project`
pub fn encode_working_dir(dir: &Path) -> String {
    let s = dir.to_string_lossy();
    let stripped = s.strip_prefix('/').unwrap_or(&s);
    stripped.replace('/', "-")
}

/// Find the most recently modified `.jsonl` file in the Claude Code projects
/// directory for the given working directory.
pub fn find_session_jsonl(working_dir: &Path) -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let encoded = encode_working_dir(working_dir);
    let projects_dir = PathBuf::from(home)
        .join(".claude")
        .join("projects")
        .join(&encoded);

    newest_jsonl_in(&projects_dir)
}

fn newest_jsonl_in(dir: &Path) -> Option<PathBuf> {
    if !dir.is_dir() {
        return None;
    }

    let mut newest: Option<(PathBuf, SystemTime)> = None;

    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let mtime = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);

        match &newest {
            Some((_, best_time)) if mtime <= *best_time => {}
            _ => newest = Some((path, mtime)),
        }
    }

    newest.map(|(p, _)| p)
}

/// Validate outputs for a node using the pipeline definition.
pub fn validate_outputs(
    pipeline_path: &Path,
    node_id: &str,
    iter: i64,
    artifacts_dir: &Path,
) -> bool {
    let yaml = match std::fs::read_to_string(pipeline_path) {
        Ok(y) => y,
        Err(_) => return false,
    };
    let pipeline_def = match pipeline::parse_pipeline(&yaml) {
        Ok(p) => p.pipeline,
        Err(_) => return false,
    };

    outputs_validator::validate(&pipeline_def, node_id, iter, artifacts_dir).is_ok()
}

/// Build events for a detection result. Returns empty vec for Detection::Ok.
pub fn detection_events(
    detection: &Detection,
    run_id: &str,
    node_id: &str,
    iter: i64,
) -> Vec<event_log::Event> {
    let (kind, reason) = match detection {
        Detection::Ok => return vec![],
        Detection::SessionDied => (EventKind::NodeFailed, "session_died"),
        Detection::AutoComplete => (EventKind::NodeAutoCompleted, "auto_completed_idle_valid"),
        Detection::Stale => (EventKind::NodeStale, "idle_outputs_incomplete"),
    };

    vec![event_log::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: event_log::now_iso(),
        kind,
        node_id: Some(node_id.to_string()),
        iter: Some(iter),
        payload: Some(serde_json::json!({ "reason": reason })),
    }]
}

/// Collect all running nodes from a RunState.
pub fn running_nodes(run_state: &event_log::RunState) -> Vec<(String, i64)> {
    run_state
        .nodes
        .iter()
        .filter(|(_, ns)| ns.status == NodeStatus::Running)
        .map(|(id, ns)| (id.clone(), ns.iter))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    // --- encode_working_dir ---

    #[test]
    fn encode_basic_path() {
        assert_eq!(
            encode_working_dir(Path::new("/home/user/project")),
            "home-user-project"
        );
    }

    #[test]
    fn encode_root() {
        assert_eq!(encode_working_dir(Path::new("/")), "");
    }

    #[test]
    fn encode_deeply_nested() {
        assert_eq!(encode_working_dir(Path::new("/a/b/c/d/e")), "a-b-c-d-e");
    }

    // --- decide (pure logic) ---

    #[test]
    fn dead_session_returns_session_died() {
        let probe = NodeProbe {
            session_alive: false,
            jsonl_mtime: None,
            now: SystemTime::now(),
            artifacts_valid: None,
        };
        assert_eq!(decide(&probe), Detection::SessionDied);
    }

    #[test]
    fn dead_session_regardless_of_mtime() {
        let now = SystemTime::now();
        let probe = NodeProbe {
            session_alive: false,
            jsonl_mtime: Some(now - Duration::from_secs(300)),
            now,
            artifacts_valid: Some(true),
        };
        assert_eq!(decide(&probe), Detection::SessionDied);
    }

    #[test]
    fn no_jsonl_file_returns_ok() {
        let probe = NodeProbe {
            session_alive: true,
            jsonl_mtime: None,
            now: SystemTime::now(),
            artifacts_valid: None,
        };
        assert_eq!(decide(&probe), Detection::Ok);
    }

    #[test]
    fn fresh_mtime_returns_ok() {
        let now = SystemTime::now();
        let probe = NodeProbe {
            session_alive: true,
            jsonl_mtime: Some(now - Duration::from_secs(60)),
            now,
            artifacts_valid: None,
        };
        assert_eq!(decide(&probe), Detection::Ok);
    }

    #[test]
    fn threshold_boundary_119s_returns_ok() {
        let now = SystemTime::now();
        let probe = NodeProbe {
            session_alive: true,
            jsonl_mtime: Some(now - Duration::from_secs(119)),
            now,
            artifacts_valid: Some(true),
        };
        assert_eq!(decide(&probe), Detection::Ok);
    }

    #[test]
    fn threshold_boundary_120s_with_valid_artifacts_returns_auto_complete() {
        let now = SystemTime::now();
        let probe = NodeProbe {
            session_alive: true,
            jsonl_mtime: Some(now - Duration::from_secs(120)),
            now,
            artifacts_valid: Some(true),
        };
        assert_eq!(decide(&probe), Detection::AutoComplete);
    }

    #[test]
    fn threshold_boundary_120s_with_invalid_artifacts_returns_stale() {
        let now = SystemTime::now();
        let probe = NodeProbe {
            session_alive: true,
            jsonl_mtime: Some(now - Duration::from_secs(120)),
            now,
            artifacts_valid: Some(false),
        };
        assert_eq!(decide(&probe), Detection::Stale);
    }

    #[test]
    fn threshold_boundary_121s_with_valid_artifacts_returns_auto_complete() {
        let now = SystemTime::now();
        let probe = NodeProbe {
            session_alive: true,
            jsonl_mtime: Some(now - Duration::from_secs(121)),
            now,
            artifacts_valid: Some(true),
        };
        assert_eq!(decide(&probe), Detection::AutoComplete);
    }

    #[test]
    fn threshold_boundary_121s_with_missing_artifacts_returns_stale() {
        let now = SystemTime::now();
        let probe = NodeProbe {
            session_alive: true,
            jsonl_mtime: Some(now - Duration::from_secs(121)),
            now,
            artifacts_valid: None,
        };
        assert_eq!(decide(&probe), Detection::Stale);
    }

    #[test]
    fn idle_with_valid_artifacts_auto_completes() {
        let now = SystemTime::now();
        let probe = NodeProbe {
            session_alive: true,
            jsonl_mtime: Some(now - Duration::from_secs(200)),
            now,
            artifacts_valid: Some(true),
        };
        assert_eq!(decide(&probe), Detection::AutoComplete);
    }

    #[test]
    fn idle_with_invalid_artifacts_is_stale() {
        let now = SystemTime::now();
        let probe = NodeProbe {
            session_alive: true,
            jsonl_mtime: Some(now - Duration::from_secs(200)),
            now,
            artifacts_valid: Some(false),
        };
        assert_eq!(decide(&probe), Detection::Stale);
    }

    // --- detection_events ---

    #[test]
    fn events_ok_is_empty() {
        assert!(detection_events(&Detection::Ok, "r", "n", 1).is_empty());
    }

    #[test]
    fn events_session_died() {
        let events = detection_events(&Detection::SessionDied, "run1", "node1", 1);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, EventKind::NodeFailed);
        assert_eq!(events[0].node_id.as_deref(), Some("node1"));
        let payload = events[0].payload.as_ref().unwrap();
        assert_eq!(payload["reason"], "session_died");
    }

    #[test]
    fn events_auto_complete() {
        let events = detection_events(&Detection::AutoComplete, "run1", "node1", 2);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, EventKind::NodeAutoCompleted);
        assert_eq!(events[0].iter, Some(2));
    }

    #[test]
    fn events_stale() {
        let events = detection_events(&Detection::Stale, "run1", "node1", 1);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, EventKind::NodeStale);
    }

    // --- running_nodes ---

    #[test]
    fn running_nodes_filters_correctly() {
        use crate::event_log::{project, Event, EventKind};

        fn make_event(kind: EventKind, node_id: Option<&str>, iter: Option<i64>) -> Event {
            let payload = if kind == EventKind::RunStarted {
                Some(serde_json::json!({ "pipeline_name": "test" }))
            } else {
                None
            };
            Event {
                id: None,
                run_id: "test-run".to_string(),
                ts: event_log::now_iso(),
                kind,
                node_id: node_id.map(String::from),
                iter,
                payload,
            }
        }

        let events = vec![
            make_event(EventKind::RunStarted, None, None),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeStarted, Some("planner"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("planner"), Some(1)),
        ];

        let state = project(&events).unwrap();
        let running = running_nodes(&state);
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].0, "worker");
        assert_eq!(running[0].1, 1);
    }

    // --- find_session_jsonl (filesystem) ---

    #[test]
    fn find_jsonl_returns_newest_file() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();

        let encoded = encode_working_dir(Path::new("/home/user/project"));
        let projects_dir = home.join(".claude").join("projects").join(&encoded);
        std::fs::create_dir_all(&projects_dir).unwrap();

        let old_file = projects_dir.join("old-session.jsonl");
        std::fs::write(&old_file, "old").unwrap();
        filetime::set_file_mtime(
            &old_file,
            filetime::FileTime::from_system_time(SystemTime::now() - Duration::from_secs(300)),
        )
        .unwrap();

        let new_file = projects_dir.join("new-session.jsonl");
        std::fs::write(&new_file, "new").unwrap();

        std::env::set_var("HOME", home);
        let result = find_session_jsonl(Path::new("/home/user/project"));

        assert!(result.is_some());
        assert_eq!(result.unwrap().file_name().unwrap(), "new-session.jsonl");
    }

    #[test]
    fn find_jsonl_no_dir_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", tmp.path());
        assert!(find_session_jsonl(Path::new("/nonexistent/dir")).is_none());
    }

    #[test]
    fn find_jsonl_ignores_non_jsonl_files() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();

        let encoded = encode_working_dir(Path::new("/tmp/testdir"));
        let projects_dir = home.join(".claude").join("projects").join(&encoded);
        std::fs::create_dir_all(&projects_dir).unwrap();

        std::fs::write(projects_dir.join("notes.txt"), "not jsonl").unwrap();
        std::fs::write(projects_dir.join("data.json"), "not jsonl either").unwrap();

        std::env::set_var("HOME", home);
        assert!(find_session_jsonl(Path::new("/tmp/testdir")).is_none());
    }

    // --- validate_outputs (integration with outputs_validator) ---

    #[test]
    fn validate_outputs_with_no_declared_outputs() {
        let tmp = tempfile::tempdir().unwrap();
        let pipeline_path = tmp.path().join("pipeline.yaml");
        std::fs::write(
            &pipeline_path,
            "name: test\nnodes:\n  - id: worker\n    type: doc-only\n    inputs: []\n    outputs: []\nedges: []\n",
        )
        .unwrap();

        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        assert!(validate_outputs(
            &pipeline_path,
            "worker",
            1,
            &artifacts_dir
        ));
    }

    #[test]
    fn validate_outputs_with_missing_artifact() {
        let tmp = tempfile::tempdir().unwrap();
        let pipeline_path = tmp.path().join("pipeline.yaml");
        std::fs::write(
            &pipeline_path,
            "name: test\nnodes:\n  - id: worker\n    type: doc-only\n    inputs: []\n    outputs:\n      - name: report\n    edges: []\n",
        )
        .unwrap();

        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        assert!(!validate_outputs(
            &pipeline_path,
            "worker",
            1,
            &artifacts_dir
        ));
    }
}
