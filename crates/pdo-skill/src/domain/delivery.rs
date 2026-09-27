//! **Delivery** of a NodeRun's skills actifs into the worktree it works in
//! (#672, spec #667, ADR-0062, CONTEXT.md §*Banque de skills* — *Livraison*).
//!
//! One mechanism for every harness: the skills are **copied** into
//! `.agents/skills/<name>/` of the worktree, with one symbolic link per skill in
//! `.claude/skills/<name>` (the only two locations `claude`, `copilot` and
//! `opencode` read without configuration — ADR-0062 measured that `claude` reads
//! `.claude/skills` but not `.agents/skills`). The price — files PDO writes into a
//! repository it does not own — is paid by a **per-skill Git exclusion**:
//!
//! - every delivered path is written to the target repo's `info/exclude`
//!   (shared by all its worktrees, never versioned), one path per skill and never a
//!   parent folder, each preceded by a `# pdo <run-id>` marker line so the Run
//!   cleanup can take exactly them back;
//! - every delivered path is also recorded in the worktree's provisioning manifest
//!   (ADR-0061), so the completion commit filters it even when the agent staged it
//!   itself with `git add -A`.
//!
//! A versioned `.agents/skills/` or `.claude/skills/` stays intact and tracked: a
//! skill **homonymous with a versioned one is skipped with a warning**, never
//! overwritten; a pre-existing `.claude/skills` (real folder or symlink) receives
//! the per-skill links beside its own content.
//!
//! **Content is frozen at the Run**: the bank's folders are copied ONCE into the
//! Run's snapshot (`<repo>/.pdo/runs/<run-id>/skills/<id>/`, outside the worktree)
//! and every delivery reads the snapshot, never the bank. The snapshot is
//! **additive** — a node whose selection changed after the Run started gets its new
//! skill snapshotted at its own spawn; nothing is ever purged during the Run.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::selection::{ActiveSkill, MissingSkill};

/// Marker written on its own line before each excluded path. The run id follows
/// so the cleanup of one Run never touches another Run's lines.
pub const EXCLUDE_MARKER: &str = "# pdo";

/// Payload key of the `RunStarted` event carrying the resolved
/// instance + Projet + Run selection (ids, bank names, tiers) frozen at create.
pub const RUN_STARTED_FROZEN_KEY: &str = "frozen_skills";

/// Where one Run's frozen skill content lives: beside `worktree/`, under the Run
/// directory (removed with it at cleanup), never inside a worktree.
pub fn snapshot_root(run_repo_root: &Path, run_id: &str) -> PathBuf {
    run_repo_root
        .join(".pdo")
        .join("runs")
        .join(run_id)
        .join("skills")
}

/// A skill the node was promised but that could not be written into the worktree.
/// Never a failure: the node runs without it and the Run view says so.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SkippedSkill {
    pub id: String,
    pub name: String,
    pub reason: String,
}

impl SkippedSkill {
    pub fn new(skill: &ActiveSkill, name: &str, reason: impl Into<String>) -> Self {
        SkippedSkill {
            id: skill.id.clone(),
            name: name.to_string(),
            reason: reason.into(),
        }
    }
}

/// What one delivery into a worktree did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeliveryReport {
    /// Names present in the worktree after the call (freshly written or already
    /// delivered by an earlier spawn into the same worktree).
    pub delivered: Vec<String>,
    pub skipped: Vec<SkippedSkill>,
}

/// Can `name` name the delivered folder? Never a path, never hidden.
pub fn usable_folder_name(name: &str) -> bool {
    !(name.is_empty() || name.contains('/') || name.contains('\\') || name.starts_with('.'))
}

/// The instance + Projet + Run selection frozen on `RunStarted`: the skills the
/// bank knew at create, and the ids it no longer had (kept so every node of the
/// Run keeps reporting them as missing, with their stored label and tiers).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrozenRunSkills {
    pub skills: Vec<ActiveSkill>,
    pub missing: Vec<MissingSkill>,
}

impl FrozenRunSkills {
    /// Read from the `RunStarted` payload. `None` for a Run created before #672
    /// (the spawn then reads those tiers fresh).
    pub fn from_run_started(payload: &serde_json::Value) -> Option<Self> {
        let skills: Option<Vec<ActiveSkill>> = payload
            .get(RUN_STARTED_FROZEN_KEY)
            .and_then(|v| serde_json::from_value(v.clone()).ok());
        let missing: Option<Vec<MissingSkill>> = payload
            .get("missing_skills")
            .and_then(|v| serde_json::from_value(v.clone()).ok());
        if skills.is_none() && missing.is_none() {
            return None;
        }
        Some(FrozenRunSkills {
            skills: skills.unwrap_or_default(),
            missing: missing.unwrap_or_default(),
        })
    }
}

/// The exclusion lines of `run_id` in an `info/exclude` text.
pub fn run_exclusions(content: &str, run_id: &str) -> Vec<String> {
    let marker = format!("{EXCLUDE_MARKER} {run_id}");
    let mut out = Vec::new();
    let mut lines = content.lines();
    while let Some(line) = lines.next() {
        if line.trim_end() == marker {
            if let Some(path) = lines.next() {
                out.push(path.trim_end().to_string());
            }
        }
    }
    out
}

/// `content` without any `# pdo <run-id>` marker and the path line it
/// introduces. Lines PDO did not write — and other Runs' lines — stay byte for
/// byte.
pub fn without_run_exclusions(content: &str, run_id: &str) -> String {
    let marker = format!("{EXCLUDE_MARKER} {run_id}");
    let mut kept: Vec<&str> = Vec::new();
    let mut lines = content.lines();
    while let Some(line) = lines.next() {
        if line.trim_end() == marker {
            lines.next();
            continue;
        }
        kept.push(line);
    }
    let mut out = kept.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// What to append to an `info/exclude` holding `existing` so each of
/// `patterns` is excluded for `run_id` exactly once. Empty when nothing is
/// missing.
pub fn exclusions_to_append(existing: &str, run_id: &str, patterns: &[String]) -> String {
    let present: BTreeSet<String> = run_exclusions(existing, run_id).into_iter().collect();
    let missing: Vec<&String> = patterns.iter().filter(|p| !present.contains(*p)).collect();
    if missing.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    if !existing.is_empty() && !existing.ends_with('\n') {
        out.push('\n');
    }
    for pattern in missing {
        out.push_str(&format!("{EXCLUDE_MARKER} {run_id}\n{pattern}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::selection::SkillTier;

    #[test]
    fn cleanup_removes_only_this_runs_marked_lines() {
        let content = "# user line\n*.swp\n# pdo other-run\n/.agents/skills/zzz/\n\
                       # pdo r6\n/.agents/skills/tdd/\n# pdo r6\n/.claude/skills/tdd\n";
        assert_eq!(
            run_exclusions(content, "r6"),
            vec!["/.agents/skills/tdd/", "/.claude/skills/tdd"]
        );
        assert_eq!(
            without_run_exclusions(content, "r6"),
            "# user line\n*.swp\n# pdo other-run\n/.agents/skills/zzz/\n"
        );
        assert_eq!(without_run_exclusions("", "r6"), "");
    }

    #[test]
    fn exclusions_are_appended_once_after_a_newline() {
        let patterns = vec!["/.agents/skills/tdd/".to_string()];
        assert_eq!(
            exclusions_to_append("*.swp", "r1", &patterns),
            "\n# pdo r1\n/.agents/skills/tdd/\n"
        );
        let already = "# pdo r1\n/.agents/skills/tdd/\n";
        assert_eq!(exclusions_to_append(already, "r1", &patterns), "");
        assert_eq!(
            exclusions_to_append(already, "r2", &patterns),
            "# pdo r2\n/.agents/skills/tdd/\n"
        );
    }

    #[test]
    fn a_folder_name_is_never_a_path_nor_hidden() {
        assert!(usable_folder_name("tdd"));
        for bad in ["", "a/b", "a\\b", ".hidden"] {
            assert!(!usable_folder_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn the_frozen_selection_reads_from_run_started() {
        let payload = serde_json::json!({
            "frozen_skills": [{"id": "a", "name": "tdd", "tiers": ["instance"]}],
        });
        let frozen = FrozenRunSkills::from_run_started(&payload).unwrap();
        assert_eq!(frozen.skills[0].tiers, vec![SkillTier::Instance]);
        assert!(frozen.missing.is_empty());
        assert_eq!(
            FrozenRunSkills::from_run_started(&serde_json::json!({})),
            None
        );
    }
}
