//! `pdo run repos`: add or remove secondary repositories of a running Run,
//! over `PATCH /runs/{id}/repos`, as the run view's Repositories section does.

use anyhow::Result;
use pdo_platform::adapters::cli::daemon::{absolute_path, segment, Daemon, NotFound};
use serde_json::{json, Value};

use super::run_gesture::value;

/// The edit, sent once every `--remove` names a secondary of the Run. The
/// route takes an absent alias for a no-op (the UI's double-click), so an
/// unknown one is refused here, or it would read « updated » and change nothing.
pub(super) fn edit_repos(
    daemon: &Daemon,
    run_id: &str,
    add: &[String],
    read_only: bool,
    remove: &[String],
) -> Result<Value> {
    if add.is_empty() && remove.is_empty() {
        anyhow::bail!("nothing to change: give --add or --remove");
    }
    let add = add
        .iter()
        .map(|repo| Ok(json!({ "repo": repo_arg(repo)?, "read_only": read_only })))
        .collect::<Result<Vec<_>>>()?;
    let remove = if remove.is_empty() {
        Vec::new()
    } else {
        let run = daemon
            .get(&format!("/runs/{}", segment(run_id)))?
            .ok()?
            .json()?;
        // A Run with no secondary leaves `target_repos` out.
        let secondaries = run["target_repos"].as_array().cloned().unwrap_or_default();
        remove
            .iter()
            .map(|raw| secondary_alias(run_id, raw, &secondaries))
            .collect::<Result<Vec<_>>>()?
    };
    value(daemon.patch(
        &format!("/runs/{}/repos", segment(run_id)),
        &json!({ "add": add, "remove": remove }),
    )?)
}

/// The alias of the Run's secondary that `raw` names: its alias, or its
/// repository (a URL as is, a path from the working directory, symlinks
/// resolved). One the Run does not have is refused, exit 4, with the Run's
/// secondaries.
fn secondary_alias(run_id: &str, raw: &str, secondaries: &[Value]) -> Result<String> {
    let raw = raw.trim();
    let alias = |pin: &Value| pin["alias"].as_str().unwrap_or_default().to_string();
    let repo = |pin: &Value| pin["repo"].as_str().unwrap_or_default().to_string();
    if let Some(pin) = secondaries.iter().find(|pin| alias(pin) == raw) {
        return Ok(alias(pin));
    }
    let wanted = repo_arg(raw)?;
    let canonical = |path: &str| std::fs::canonicalize(path).ok();
    let found = secondaries.iter().find(|pin| {
        let pinned = repo(pin);
        pinned == wanted || canonical(&pinned).is_some_and(|c| Some(c) == canonical(&wanted))
    });
    if let Some(pin) = found {
        return Ok(alias(pin));
    }
    let known = if secondaries.is_empty() {
        "it has none".to_string()
    } else {
        let list: Vec<String> = secondaries
            .iter()
            .map(|pin| format!("{} ({})", alias(pin), repo(pin)))
            .collect();
        format!("its secondaries: {}", list.join(", "))
    };
    Err(NotFound(format!(
        "{raw} is not a secondary repository of run {run_id}; {known}"
    ))
    .into())
}

/// A repository argument as the UI sends it: a URL as is, a path absolute.
fn repo_arg(raw: &str) -> Result<String> {
    if raw.contains("://") || raw.starts_with("git@") {
        return Ok(raw.to_string());
    }
    absolute_path(raw)
}
