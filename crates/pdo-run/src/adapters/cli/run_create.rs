//! `pdo run create` (#721, ADR-0064): the thin CLI client of the daemon's
//! `POST /runs`. Blocking `reqwest`, no tokio runtime; plain `Result` →
//! `0`/`1` exit mapping.

use std::time::Duration;

use anyhow::{Context, Result};
use pdo_kernel::actor::{SESSION_NODE_HEADER, SESSION_RUN_HEADER};

use super::run_action::RunAction;
use super::upload::{
    intermediary_refusal_detail, run_create_multipart, upload_timeout, upload_transport_error,
};
use super::{cli_daemon_url, session_env_claim};

/// Parse a JSON-typed flag. The error names the flag, so an agent reading the
/// stderr knows which argument was malformed.
fn parse_json_arg(flag: &str, raw: &str) -> Result<serde_json::Value> {
    serde_json::from_str(raw).with_context(|| format!("invalid JSON for --{flag}: {raw}"))
}

/// `--skills`: comma-separated ids (`a,b`) or a JSON list of `{id, name}`
/// objects. Ids become `{id}` objects — the wire shape of the Run tier's
/// `SkillRef` list (`name` defaults; #668: identity is the id).
pub(crate) fn parse_skills_arg(raw: &str) -> Result<serde_json::Value> {
    let trimmed = raw.trim();
    if trimmed.starts_with('[') {
        return parse_json_arg("skills", trimmed);
    }
    let refs: Vec<serde_json::Value> = trimmed
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|id| serde_json::json!({ "id": id }))
        .collect();
    Ok(serde_json::json!(refs))
}

/// `--ext <id>.<key>=<value>` (repeatable): the `extensions` object of `POST
/// /runs`, `{"<id>": {"<key>": "<value>", …}, …}`. The value is a string, split
/// at the first `=` only (it may hold `=` and `.`); the id at the first `.`.
/// The same key given twice is refused rather than one silently dropped.
pub(crate) fn parse_ext_args(raw: &[String]) -> Result<serde_json::Value> {
    let mut extensions = serde_json::Map::new();
    for arg in raw {
        let malformed = || anyhow::anyhow!("invalid --ext {arg}: expected <id>.<key>=<value>");
        let (path, value) = arg.split_once('=').ok_or_else(malformed)?;
        let (id, key) = path.split_once('.').ok_or_else(malformed)?;
        let (id, key) = (id.trim(), key.trim());
        if id.is_empty() || key.is_empty() {
            return Err(malformed());
        }
        let entry = extensions
            .entry(id.to_string())
            .or_insert_with(|| serde_json::json!({}));
        let object = entry.as_object_mut().expect("built as an object");
        if object.contains_key(key) {
            anyhow::bail!("--ext {id}.{key} given twice");
        }
        object.insert(key.to_string(), serde_json::json!(value));
    }
    Ok(serde_json::Value::Object(extensions))
}

/// The flag a refusal naming the Projets of the repository asks for (its
/// `projects`, a repository in several of them and none named): ` Pass
/// --project <id>: <name> (<id>), …`. `None` for any other refusal.
fn project_hint(raw: &str) -> Option<String> {
    let body: serde_json::Value = serde_json::from_str(raw).ok()?;
    let projects = body.get("projects")?.as_array()?;
    let named: Vec<String> = projects
        .iter()
        .filter_map(|p| Some(format!("{} ({})", p["name"].as_str()?, p["id"].as_str()?)))
        .collect();
    if named.is_empty() {
        return Some(" Pass --project <id>.".to_string());
    }
    Some(format!(" Pass --project <id>: {}.", named.join(", ")))
}

/// One-shot `pdo run create` (issue #721, ADR-0064): the thin CLI client of the
/// daemon's `POST /runs`. Like the other one-shots, blocking `reqwest` and no
/// tokio runtime (see `crates/pdo/src/entry.rs`); plain `Result` → `0`/`1` exit mapping — only
/// `pdo complete` owns an exit-code contract (#490).
///
/// The body is built field by field rather than by serialising a struct, so the
/// pass-through stays visibly in lock-step with the daemon's
/// `CREATE_RUN_FIELDS` — and provenance never enters it: identity travels in
/// the session headers, exactly like the UI's fetch calls.
pub fn run_run_create(action: RunAction) -> Result<()> {
    let RunAction::Create {
        pipeline,
        input,
        input_file,
        image,
        file,
        variables,
        skills,
        agent_choice,
        harness,
        sandbox,
        target_repo,
        project,
        target_repos,
        source_branch,
        name,
        auto_name,
        auto_fail,
        provisioning,
        ext,
    } = action
    else {
        anyhow::bail!("`pdo run create` only: `wait` and `list` have their own entry points");
    };

    let url = cli_daemon_url();

    // #779: `--input-file` reads the prompt from disk (clap already refuses it
    // together with `--input`). Read eagerly so a missing file fails before
    // the daemon is even reached.
    let input = match input_file {
        Some(path) => std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read --input-file {}", path.display()))?,
        None => input,
    };

    let mut body = serde_json::Map::new();
    body.insert("pipeline".into(), serde_json::json!(pipeline));
    body.insert("input".into(), serde_json::json!(input));
    if let Some(v) = variables {
        body.insert("variables".into(), parse_json_arg("variables", &v)?);
    }

    if let Some(v) = skills {
        body.insert("skills".into(), parse_skills_arg(&v)?);
    }
    if let Some(v) = agent_choice {
        body.insert("agent_choice".into(), parse_json_arg("agent-choice", &v)?);
    }
    if let Some(v) = harness {
        // No validation here (ADR-0045) — the daemon refuses an unknown one at
        // spawn, naming it.
        body.insert("harness".into(), serde_json::json!(v));
    }
    if let Some(v) = sandbox {
        body.insert("sandbox".into(), serde_json::json!(v));
    }
    if let Some(v) = target_repo {
        body.insert("target_repo".into(), serde_json::json!(v));
    }
    if let Some(v) = project {
        body.insert("project_id".into(), serde_json::json!(v));
    }
    if let Some(v) = target_repos {
        body.insert("target_repos".into(), parse_json_arg("target-repos", &v)?);
    }
    if let Some(v) = source_branch {
        body.insert("source_branch".into(), serde_json::json!(v));
    }
    if let Some(v) = name {
        body.insert("name".into(), serde_json::json!(v));
    }
    if let Some(v) = auto_name {
        body.insert("auto_name".into(), serde_json::json!(v));
    }
    if let Some(v) = auto_fail {
        body.insert("auto_fail".into(), serde_json::json!(v));
    }
    if let Some(v) = provisioning {
        body.insert("provisioning".into(), parse_json_arg("provisioning", &v)?);
    }
    if !ext.is_empty() {
        body.insert("extensions".into(), parse_ext_args(&ext)?);
    }

    let session = session_env_claim();
    // #779: any `--image` / `--file` switches the body to multipart — the same
    // text fields as the JSON shape (non-scalars as JSON strings, like the UI),
    // plus one `images` / `files` part per attachment. Files are read here so a
    // missing path is named locally, before any request.
    let (form, attachment_bytes) = if image.is_empty() && file.is_empty() {
        (None, 0)
    } else {
        let (form, bytes) = run_create_multipart(&body, &image, &file)?;
        (Some(form), bytes)
    };
    // #839: a bounded wait. Without it a stalled upload hangs the command
    // forever; with it a slow link still gets a budget that grows with the
    // payload, and the failure names what was being sent and to whom.
    let timeout = upload_timeout(attachment_bytes);
    let client = reqwest::blocking::Client::builder()
        .default_headers(pdo_kernel::env::cli_headers())
        .connect_timeout(Duration::from_secs(10))
        .timeout(timeout)
        .build()
        .context("failed to build the HTTP client")?
        .post(format!("{url}/runs"));
    let mut req = match form {
        Some(form) => client.multipart(form),
        None => client.json(&body),
    };
    if let Some((run_id, node_id)) = &session {
        req = req
            .header(SESSION_RUN_HEADER, run_id)
            .header(SESSION_NODE_HEADER, node_id);
    }
    let resp = req
        .send()
        .map_err(|e| upload_transport_error(&url, attachment_bytes, timeout, &e))?;

    let status = resp.status();
    let raw = resp.text().unwrap_or_default();
    if !status.is_success() {
        // Readable: surface the daemon's own `error` sentence when there is one
        // (unknown pipeline, refused session claim, …). A body that is not the
        // daemon's JSON came from something in front of it (#839): say so, and
        // for a 413 name the proxy body limit rather than PDO's own budget.
        // An admission refusal (401) says `sign in first` in its `message`.
        let detail = super::run_list::daemon_sentence(&raw).unwrap_or_else(|| {
            intermediary_refusal_detail(&url, status.as_u16(), attachment_bytes, &raw)
        });
        let hint = project_hint(&raw).unwrap_or_default();
        anyhow::bail!("daemon refused run creation ({status}): {detail}{hint}");
    }
    let parsed: serde_json::Value = serde_json::from_str(&raw)
        .with_context(|| format!("unreadable success body from daemon: {raw}"))?;
    let run_id = parsed
        .get("run_id")
        .and_then(|v| v.as_str())
        .with_context(|| format!("`run_id` missing from daemon response: {raw}"))?
        .to_string();

    match session {
        Some((parent_run, parent_node)) => {
            println!("Run {run_id} created — child of run {parent_run} (node {parent_node}).")
        }
        None => println!("Run {run_id} created (root run)."),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_naming_the_projets_of_the_repository_asks_for_one() {
        let raw = r#"{"error":"This repository is in Alpha and Beta: pick its Project.",
            "projects":[{"id":"p1","name":"Alpha"},{"id":"p2","name":"Beta"}]}"#;
        assert_eq!(
            project_hint(raw).as_deref(),
            Some(" Pass --project <id>: Alpha (p1), Beta (p2).")
        );
        assert_eq!(
            project_hint(r#"{"error":"x","projects":[]}"#).as_deref(),
            Some(" Pass --project <id>.")
        );
        assert_eq!(project_hint(r#"{"error":"unknown project `p`"}"#), None);
        assert_eq!(project_hint("<html>"), None);
    }

    #[test]
    fn run_create_skills_arg_accepts_ids_and_json() {
        // Comma-separated ids become the wire shape of the Run tier's SkillRef
        // list — `{id}` objects whose `name` the daemon defaults (#668).
        let parsed = parse_skills_arg("alpha, beta ,,gamma").unwrap();
        assert_eq!(
            parsed,
            serde_json::json!([{ "id": "alpha" }, { "id": "beta" }, { "id": "gamma" }])
        );
        // A JSON list passes through verbatim — `{id, name}` labels survive.
        let parsed = parse_skills_arg(r#"[{"id":"a","name":"A"}]"#).unwrap();
        assert_eq!(parsed, serde_json::json!([{ "id": "a", "name": "A" }]));
        assert!(parse_skills_arg("[broken").is_err());
    }

    #[test]
    fn ext_flags_fill_the_extensions_object() {
        let raw: Vec<String> = ["test.cle=valeur", "test.autre=a=b.c", "test2.x="]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            parse_ext_args(&raw).unwrap(),
            serde_json::json!({
                "test": { "cle": "valeur", "autre": "a=b.c" },
                "test2": { "x": "" },
            })
        );
        for bad in ["test", "test=v", ".k=v", "test.=v"] {
            let err = parse_ext_args(&[bad.to_string()]).unwrap_err().to_string();
            assert!(err.contains("expected <id>.<key>=<value>"), "{bad}: {err}");
        }
        let twice = parse_ext_args(&["t.k=1".into(), "t.k=2".into()]).unwrap_err();
        assert_eq!(twice.to_string(), "--ext t.k given twice");
    }
}
