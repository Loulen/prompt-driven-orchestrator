//! Stage 4 (ADR-0080) — **every mutation route of an extracted context goes
//! through the Autorisation d'opération** (#930).
//!
//! The routes are read from the routers themselves (`route_coverage.rs`'s
//! reader, over the context routers it lists), so a context extracted later
//! inherits the check: its mutation routes (`POST`, `PUT`, `PATCH`, `DELETE`)
//! join the enumeration, and each needs a request below. With an extension that
//! refuses everything, every one of them answers `403 operation_refused`.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::{json, Value};

use crate::common::flow::git_init;
use crate::common::TestDaemon;
use crate::route_coverage::{context_routes, Route};
use crate::test_extension::{Refusal, TestExtension};

const MUTATIONS: [&str; 4] = ["POST", "PUT", "PATCH", "DELETE"];

/// The `POST`s that mutate nothing, with why.
const NOT_MUTATIONS: &[(&str, &str, &str)] = &[
    (
        "POST",
        "/repos/provisioning/preview",
        "computes the plan of a worktree provisioning; stores nothing",
    ),
    (
        "POST",
        "/library/{name}/instantiate",
        "reads a library entry to drop onto a canvas; stores nothing",
    ),
    (
        "POST",
        "/nodes/parse",
        "parses a pasted node into a canvas spec; stores nothing",
    ),
];

/// One request aimed at a route: its method, its template, the concrete path
/// and the body that gets it past extraction.
struct Call {
    method: &'static str,
    template: &'static str,
    path: String,
    body: Option<Value>,
}

fn call(method: &'static str, template: &'static str, path: String, body: Option<Value>) -> Call {
    Call {
        method,
        template,
        path,
        body,
    }
}

/// A valid request per mutation route, over fixtures `seed` created while the
/// extension still allowed everything.
fn calls(repo: &Path, fixtures: &Fixtures) -> Vec<Call> {
    let repo = repo.to_string_lossy().to_string();
    let q = format!("path={repo}");
    let project = &fixtures.project;
    let site = fixtures.site.to_string_lossy().to_string();
    let skill = &fixtures.skill;
    let folder = &fixtures.folder;
    let pipeline = &fixtures.pipeline;
    let entry = &fixtures.library_entry;
    vec![
        call(
            "POST",
            "/pipelines",
            "/pipelines".into(),
            Some(json!({ "name": "another" })),
        ),
        call(
            "PUT",
            "/pipelines/{pipeline_id}",
            format!("/pipelines/{pipeline}"),
            Some(json!({ "yaml": PIPELINE_EDIT, "prompts": {} })),
        ),
        call(
            "DELETE",
            "/pipelines/{pipeline_id}",
            format!("/pipelines/{pipeline}"),
            None,
        ),
        call(
            "POST",
            "/pipelines/{pipeline_id}/duplicate",
            format!("/pipelines/{pipeline}/duplicate"),
            None,
        ),
        call(
            "PUT",
            "/pipelines/{pipeline_id}/rename",
            format!("/pipelines/{pipeline}/rename"),
            Some(json!({ "name": "renamed" })),
        ),
        call(
            "POST",
            "/pipelines/import",
            "/pipelines/import".into(),
            Some(json!({ "document": "pdo_pipeline: 1\npipeline: {}\n" })),
        ),
        call(
            "POST",
            "/library",
            "/library".into(),
            Some(json!({ "name": "another", "type": "agent", "prompt": "p" })),
        ),
        call(
            "DELETE",
            "/library/{name}",
            format!("/library/{entry}"),
            None,
        ),
        call(
            "POST",
            "/library/import",
            "/library/import".into(),
            Some(json!({ "filename": "flow.js", "content": "agent(`x`, { label: 'a' })" })),
        ),
        call(
            "POST",
            "/pages",
            "/pages".into(),
            Some(json!({ "name": "other", "directory": site })),
        ),
        call("DELETE", "/pages/{name}", "/pages/site".into(), None),
        call("POST", "/update/check", "/update/check".into(), None),
        call("POST", "/update/apply", "/update/apply".into(), None),
        call("POST", "/repos/fetch", format!("/repos/fetch?{q}"), None),
        call(
            "POST",
            "/repos/fast-forward",
            format!("/repos/fast-forward?{q}"),
            Some(json!({ "branch": "main" })),
        ),
        call(
            "POST",
            "/repos/create",
            "/repos/create".into(),
            Some(json!({ "parent": fixtures.scratch, "name": "fresh" })),
        ),
        call(
            "PUT",
            "/settings/provisioning",
            "/settings/provisioning".into(),
            Some(json!({})),
        ),
        call(
            "POST",
            "/projects",
            "/projects".into(),
            Some(json!({ "name": "another" })),
        ),
        call(
            "PATCH",
            "/projects/{project_id}",
            format!("/projects/{project}"),
            Some(json!({ "name": "renamed" })),
        ),
        call(
            "DELETE",
            "/projects/{project_id}",
            format!("/projects/{project}"),
            None,
        ),
        call(
            "POST",
            "/projects/{project_id}/members",
            format!("/projects/{project}/members"),
            Some(json!({ "path": fixtures.scratch })),
        ),
        call(
            "DELETE",
            "/projects/{project_id}/members",
            format!("/projects/{project}/members"),
            Some(json!({ "path": repo })),
        ),
        call(
            "PUT",
            "/projects/{project_id}/provisioning",
            format!("/projects/{project}/provisioning"),
            Some(json!({})),
        ),
        // The Run's surfaces (#925): each asks before it touches anything.
        call(
            "POST",
            "/runs",
            "/runs".into(),
            Some(json!({ "pipeline": "fixture-pipe", "input": "go", "target_repo": repo })),
        ),
        call("DELETE", "/runs/{run_id}", format!("/runs/{RUN}"), None),
        call(
            "PATCH",
            "/runs/{run_id}/repos",
            format!("/runs/{RUN}/repos"),
            Some(json!({})),
        ),
        call(
            "PUT",
            "/runs/{run_id}/pipeline",
            format!("/runs/{RUN}/pipeline"),
            Some(json!({ "yaml": PIPELINE_EDIT })),
        ),
        call(
            "POST",
            "/runs/{run_id}/manager/start",
            format!("/runs/{RUN}/manager/start"),
            None,
        ),
        call(
            "POST",
            "/runs/{run_id}/manager/stop",
            format!("/runs/{RUN}/manager/stop"),
            None,
        ),
        // The trigger context (#927): every gesture, "Run now" and the guard's
        // dry-run included, refuses before looking the Trigger up.
        call(
            "POST",
            "/triggers",
            "/triggers".into(),
            Some(json!({ "name": "nightly", "pipeline_id": "fixture-pipe", "cron": "0 3 * * *" })),
        ),
        call(
            "PATCH",
            "/triggers/{trigger_id}",
            format!("/triggers/{TRIGGER}"),
            Some(json!({ "enabled": false })),
        ),
        call(
            "DELETE",
            "/triggers/{trigger_id}",
            format!("/triggers/{TRIGGER}"),
            None,
        ),
        call(
            "POST",
            "/triggers/{trigger_id}/fire",
            format!("/triggers/{TRIGGER}/fire"),
            None,
        ),
        call(
            "POST",
            "/triggers/pause",
            "/triggers/pause".into(),
            Some(json!({ "paused": true })),
        ),
        call(
            "POST",
            "/triggers/guard/test",
            "/triggers/guard/test".into(),
            Some(json!({ "guard_command": "echo hi", "target_repo": repo })),
        ),
        // The review's gestures (#926) refuse before looking the Run up.
        call(
            "POST",
            "/runs/{run_id}/review/comments/send",
            format!("/runs/{RUN}/review/comments/send"),
            Some(
                json!({ "comments": [{ "path": "a.rs", "side": "new", "line": 1, "text": "why?" }] }),
            ),
        ),
        call(
            "POST",
            "/runs/{run_id}/review/comments/{comment_id}/reply",
            format!("/runs/{RUN}/review/comments/rc-001/reply"),
            Some(json!({ "text": "done", "resolved": true })),
        ),
        call(
            "POST",
            "/runs/{run_id}/review/comments/{comment_id}/resolve",
            format!("/runs/{RUN}/review/comments/rc-001/resolve"),
            Some(json!({})),
        ),
        call(
            "POST",
            "/runs/{run_id}/review/comments/{comment_id}/reopen",
            format!("/runs/{RUN}/review/comments/rc-001/reopen"),
            Some(json!({})),
        ),
        // The run context's gestures (#924) refuse before looking the Run up.
        call(
            "POST",
            "/runs/{run_id}/commands",
            format!("/runs/{RUN}/commands"),
            Some(json!({ "kind": "pause_run" })),
        ),
        call(
            "POST",
            "/runs/{run_id}/nodes/{node_id}/done",
            format!("/runs/{RUN}/nodes/worker/done"),
            Some(json!({ "iter": 1 })),
        ),
        call(
            "POST",
            "/runs/{run_id}/nodes/{node_id}/release",
            format!("/runs/{RUN}/nodes/worker/release"),
            Some(json!({ "iter": 1 })),
        ),
        call(
            "POST",
            "/runs/{run_id}/nodes/{node_id}/fail",
            format!("/runs/{RUN}/nodes/worker/fail"),
            Some(json!({ "reason": "broken", "iter": 1 })),
        ),
        call(
            "POST",
            "/runs/{run_id}/nodes/{node_id}/skip",
            format!("/runs/{RUN}/nodes/worker/skip"),
            Some(json!({ "reason": "nothing to do", "iter": 1 })),
        ),
        call(
            "POST",
            "/runs/{run_id}/nodes/{node_id}/start",
            format!("/runs/{RUN}/nodes/worker/start"),
            None,
        ),
        call(
            "POST",
            "/runs/{run_id}/nodes/{node_id}/stop",
            format!("/runs/{RUN}/nodes/worker/stop"),
            None,
        ),
        call(
            "POST",
            "/runs/{run_id}/nodes/{node_id}/retry",
            format!("/runs/{RUN}/nodes/worker/retry"),
            None,
        ),
        call(
            "POST",
            "/runs/{run_id}/nodes/{node_id}/wait-user",
            format!("/runs/{RUN}/nodes/worker/wait-user"),
            Some(json!({ "iter": 1 })),
        ),
        call(
            "POST",
            "/sessions/{session_id}/attach",
            format!("/sessions/pdo-{RUN}-worker/attach"),
            None,
        ),
        call(
            "POST",
            "/sessions/{run_id}/manager/attach",
            format!("/sessions/{RUN}/manager/attach"),
            None,
        ),
        call(
            "POST",
            "/sessions/{run_id}/shell",
            format!("/sessions/{RUN}/shell"),
            None,
        ),
        call(
            "POST",
            "/sessions/libassist",
            "/sessions/libassist".into(),
            None,
        ),
        call(
            "DELETE",
            "/sessions/libassist",
            "/sessions/libassist".into(),
            None,
        ),
        call(
            "PUT",
            "/sessions/libassist/focus",
            "/sessions/libassist/focus".into(),
            Some(json!({ "pipeline_id": "other" })),
        ),
        // Saves the focused template: `seed` declared the focus.
        call(
            "POST",
            "/sessions/libassist/save",
            "/sessions/libassist/save".into(),
            Some(json!({ "yaml": "name: review\n" })),
        ),
        call(
            "PUT",
            "/settings/sandbox-profiles/{name}",
            "/settings/sandbox-profiles/team".into(),
            Some(json!({})),
        ),
        call(
            "DELETE",
            "/settings/sandbox-profiles/{name}",
            "/settings/sandbox-profiles/team".into(),
            None,
        ),
        call(
            "POST",
            "/settings/skills",
            "/settings/skills".into(),
            Some(json!({ "content": skill_md("another") })),
        ),
        call(
            "PUT",
            "/settings/skills/{id}",
            format!("/settings/skills/{skill}"),
            Some(json!({ "name": "renamed" })),
        ),
        call(
            "DELETE",
            "/settings/skills/{id}",
            format!("/settings/skills/{skill}"),
            None,
        ),
        call(
            "POST",
            "/settings/skills/{id}/files",
            format!("/settings/skills/{skill}/files"),
            Some(json!({ "from_path": fixtures.host_file })),
        ),
        call(
            "PUT",
            "/settings/skills/{id}/files/{*path}",
            format!("/settings/skills/{skill}/files/{SKILL_FILE}"),
            Some(json!("changed")),
        ),
        call(
            "DELETE",
            "/settings/skills/{id}/files/{*path}",
            format!("/settings/skills/{skill}/files/{SKILL_FILE}"),
            None,
        ),
        call(
            "POST",
            "/settings/skill-folders",
            "/settings/skill-folders".into(),
            Some(json!({ "name": "another" })),
        ),
        call(
            "PUT",
            "/settings/skill-folders/{id}",
            format!("/settings/skill-folders/{folder}"),
            Some(json!({ "name": "renamed" })),
        ),
        call(
            "DELETE",
            "/settings/skill-folders/{id}",
            format!("/settings/skill-folders/{folder}"),
            None,
        ),
        // The Source routes refuse before any clone: nothing leaves the host.
        call(
            "POST",
            "/settings/skills/scan",
            "/settings/skills/scan".into(),
            Some(json!({ "scan_id": "scan-1", "source": SOURCE })),
        ),
        call(
            "POST",
            "/settings/skills/scan/{scan_id}/cancel",
            "/settings/skills/scan/scan-1/cancel".into(),
            None,
        ),
        call(
            "POST",
            "/settings/skills/import",
            "/settings/skills/import".into(),
            Some(json!({ "scan_id": "scan-1", "source": SOURCE, "items": [] })),
        ),
        call(
            "POST",
            "/settings/skill-folders/{id}/rescan",
            format!("/settings/skill-folders/{folder}/rescan"),
            Some(json!({ "scan_id": "scan-1" })),
        ),
        call(
            "POST",
            "/settings/skill-folders/{id}/update",
            format!("/settings/skill-folders/{folder}/update"),
            Some(json!({ "scan_id": "scan-1", "items": [] })),
        ),
        // The stats context (#928): the absorptions and the price sync ask
        // before they read anything, let alone reach the network.
        call(
            "POST",
            "/stats/absorptions",
            "/stats/absorptions".into(),
            Some(json!({
                "dimension": "pipeline",
                "absorbent": { "key": "keeper", "name": "Keeper" },
                "members": [{ "key": "other", "name": "Other" }],
            })),
        ),
        call(
            "DELETE",
            "/stats/absorptions/member",
            "/stats/absorptions/member?dimension=pipeline&member=other".into(),
            None,
        ),
        call(
            "POST",
            "/settings/cost-prices/sync",
            "/settings/cost-prices/sync".into(),
            None,
        ),
        // The settings context (#929): an edit of the instance settings and the
        // agentic profiles' gestures ask before they validate or read anything.
        call(
            "PUT",
            "/settings",
            "/settings".into(),
            Some(json!({ "session_cap": 3 })),
        ),
        call(
            "POST",
            "/settings/agent-profiles",
            "/settings/agent-profiles".into(),
            Some(json!({ "name": "Reviewer", "harness": "claude" })),
        ),
        call(
            "PUT",
            "/settings/agent-profiles/{id}",
            "/settings/agent-profiles/default".into(),
            Some(json!({ "name": "House", "harness": "claude" })),
        ),
        call(
            "DELETE",
            "/settings/agent-profiles/{id}",
            "/settings/agent-profiles/default".into(),
            None,
        ),
    ]
}

/// A Run id the session, run and review routes aim at: they refuse before looking it up.
const RUN: &str = "20260101-000000-abcdef0";

/// A Trigger id the trigger routes aim at: they refuse before looking it up.
const TRIGGER: &str = "trg-20260101-000000-abcdef0";

/// The reference file of the seeded skill.
const SKILL_FILE: &str = "notes.md";
const SKILL_FILE_TEXT: &str = "keep me";

/// A Source the import routes name.
const SOURCE: &str = "https://github.com/acme/skills";

fn skill_md(name: &str) -> String {
    format!("---\nname: {name}\ndescription: The {name} method.\n---\n\n# {name}\n")
}

/// The pipeline the save route would write, had it been allowed.
const PIPELINE_EDIT: &str = "name: fixture-pipe\nversion: \"2.0\"\nnodes:\n  - id: start\n    name: Start\n    type: start\n    outputs:\n      - name: user_prompt\n  - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\n";

struct Fixtures {
    pipeline: String,
    library_entry: String,
    project: String,
    site: std::path::PathBuf,
    scratch: String,
    skill: String,
    folder: String,
    host_file: String,
}

/// A Projet with the daemon's repo as member, a mounted page, the assistant's
/// focus on a template, and a skill with a reference file in a bank folder.
async fn seed(daemon: &TestDaemon, scratch: &Path) -> Fixtures {
    let client = reqwest::Client::new();
    let url = daemon.url();
    let project: Value = client
        .post(format!("{url}/projects"))
        .json(&json!({ "name": "fixture" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let project = project["id"].as_str().unwrap().to_string();
    let member = client
        .post(format!("{url}/projects/{project}/members"))
        .json(&json!({ "path": daemon.repo_root() }))
        .send()
        .await
        .unwrap();
    assert!(member.status().is_success(), "{}", member.status());

    let site = scratch.join("site");
    std::fs::create_dir_all(&site).unwrap();
    std::fs::write(site.join("index.html"), "<p>hi</p>").unwrap();
    let mount = client
        .post(format!("{url}/pages"))
        .json(&json!({ "name": "site", "directory": site }))
        .send()
        .await
        .unwrap();
    assert_eq!(mount.status(), 201);
    let focus = client
        .put(format!("{url}/sessions/libassist/focus"))
        .json(&json!({ "pipeline_id": "review" }))
        .send()
        .await
        .unwrap();
    assert!(focus.status().is_success(), "{}", focus.status());

    let folder: Value = client
        .post(format!("{url}/settings/skill-folders"))
        .json(&json!({ "name": "fixture" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let folder = folder["id"].as_str().unwrap().to_string();
    let skill: Value = client
        .post(format!("{url}/settings/skills"))
        .json(&json!({ "content": skill_md("fixture"), "folder_id": folder }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let skill = skill["id"].as_str().unwrap().to_string();
    let host_file = scratch.join("cheatsheet.md");
    std::fs::write(&host_file, SKILL_FILE_TEXT).unwrap();
    let file = client
        .post(format!("{url}/settings/skills/{skill}/files"))
        .json(&json!({ "from_path": host_file, "path": SKILL_FILE }))
        .send()
        .await
        .unwrap();
    assert_eq!(file.status(), 201);

    let pipeline: Value = client
        .post(format!("{url}/pipelines"))
        .json(&json!({ "name": "fixture-pipe" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    // The node library lives under the real `$HOME`: a unique name, taken out
    // again by the test itself.
    let library_entry = format!("ext-922-{}", uuid::Uuid::new_v4().simple());
    let starred = client
        .post(format!("{url}/library"))
        .json(&json!({ "name": library_entry, "type": "agent", "prompt": "p" }))
        .send()
        .await
        .unwrap();
    assert_eq!(starred.status(), 201);

    Fixtures {
        pipeline: pipeline["id"].as_str().unwrap().to_string(),
        library_entry,
        project,
        site,
        scratch: scratch.to_string_lossy().to_string(),
        skill,
        folder,
        host_file: host_file.to_string_lossy().to_string(),
    }
}

/// The mutation routes of the extracted contexts, read from their routers.
fn mutation_routes() -> BTreeSet<Route> {
    context_routes()
        .into_iter()
        .filter(|(method, _)| MUTATIONS.contains(&method.as_str()))
        .filter(|(method, path)| {
            !NOT_MUTATIONS
                .iter()
                .any(|(m, p, _)| m == method && p == path)
        })
        .collect()
}

#[tokio::test]
async fn an_extension_refusing_everything_refuses_every_mutation_route() {
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    let daemon = TestDaemon::spawn_with_extensions(git_init, ext.boxed())
        .await
        .unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let fixtures = seed(&daemon, scratch.path()).await;
    let calls = calls(daemon.repo_root(), &fixtures);

    // Every mutation route has its request, and nothing else does.
    let aimed: BTreeSet<Route> = calls
        .iter()
        .map(|c| (c.method.to_string(), c.template.to_string()))
        .collect();
    assert_eq!(
        aimed,
        mutation_routes(),
        "a mutation route of a context has no request here (or one aims at no route)"
    );

    control.set(Refusal::Everything);
    let client = reqwest::Client::new();
    let mut not_refused = Vec::new();
    for c in &calls {
        let method = reqwest::Method::from_bytes(c.method.as_bytes()).unwrap();
        let mut request = client.request(method, format!("{}{}", daemon.url(), c.path));
        if let Some(body) = &c.body {
            request = request.json(body);
        }
        let resp = request.send().await.unwrap();
        let status = resp.status().as_u16();
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        if status != 403 || body["error"] != "operation_refused" {
            not_refused.push(format!("{} {} → {status} {body}", c.method, c.path));
        }
    }
    assert_eq!(not_refused, Vec::<String>::new());

    // Nothing moved.
    assert_eq!(
        daemon.get_json("/runs").await,
        json!([]),
        "no Run was created"
    );
    let project = daemon
        .get_json(&format!("/projects/{}", fixtures.project))
        .await;
    assert_eq!(project["name"], "fixture");
    let pages = daemon.get_json("/pages").await;
    assert_eq!(pages.as_array().map(Vec::len), Some(1), "{pages}");
    let focus = daemon.get_json("/sessions/libassist/focus").await;
    assert_eq!(focus["pipeline_id"], "review", "{focus}");
    let absorptions = daemon.get_json("/stats/absorptions").await;
    assert_eq!(
        absorptions["absorptions"],
        json!([]),
        "no absorption was written"
    );
    let bank = daemon.get_json("/settings/skills").await;
    let entries = |key: &str| -> Vec<(Value, Value)> {
        bank[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| (entry["id"].clone(), entry["name"].clone()))
            .collect()
    };
    let (skills, folders) = (entries("skills"), entries("folders"));
    assert!(
        skills.contains(&(json!(fixtures.skill), json!("fixture"))),
        "{bank}"
    );
    assert!(
        folders.contains(&(json!(fixtures.folder), json!("fixture"))),
        "{bank}"
    );
    assert!(
        !skills
            .iter()
            .chain(&folders)
            .any(|(_, name)| name == "another"),
        "{bank}"
    );
    let pipelines = daemon.get_json("/pipelines").await;
    let ids: Vec<&str> = pipelines
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&fixtures.pipeline.as_str()), "{pipelines}");
    for created in ["another", "renamed", "fixture-pipe-copy", "flow"] {
        assert!(!ids.contains(&created), "{created}: {pipelines}");
    }
    let pipeline = daemon
        .get_json(&format!("/pipelines/{}", fixtures.pipeline))
        .await;
    assert_eq!(pipeline["pipeline"]["version"], "1.0", "{pipeline}");
    let library = daemon.get_json("/library").await;
    assert!(
        library
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["name"] == fixtures.library_entry.as_str()),
        "{library}"
    );
    control.set(Refusal::Nothing);
    let unstarred = reqwest::Client::new()
        .delete(format!(
            "{}/library/{}",
            daemon.url(),
            fixtures.library_entry
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(unstarred.status(), 204);

    let file = daemon
        .get_json(&format!(
            "/settings/skills/{}/files/{SKILL_FILE}",
            fixtures.skill
        ))
        .await;
    assert_eq!(file["text"], SKILL_FILE_TEXT, "{file}");
    let skill = daemon
        .get_json(&format!("/settings/skills/{}", fixtures.skill))
        .await;
    assert_eq!(skill["files"].as_array().map(Vec::len), Some(1), "{skill}");
}
