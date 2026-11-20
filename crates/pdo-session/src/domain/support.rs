//! The Agent de support: an interactive agent the core holds outside any Run,
//! one per Actor, with the rights of its Actor. What it reads first (the
//! embedded prompt, then the person's first question), where its folder is, and
//! what that folder remembers across a restart of the daemon.

use std::path::{Path, PathBuf};

use pdo_harness::domain::agent_choice::AgentChoice;
use serde::{Deserialize, Serialize};

/// The prompt every Agent de support reads first, embedded in the binary: its
/// role, the confirmation it asks before any change, where it looks.
pub const SUPPORT_AGENT_PROMPT: &str = include_str!("../../../../prompts/builtin/support-agent.md");

/// What an Agent de support was opened with: the choice of agent, and what it
/// resolved to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupportAgent {
    pub choice: AgentChoice,
    /// The profile's name, or `Custom` for an inline combination.
    pub label: String,
    pub harness: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// RFC 3339.
    pub opened_at: String,
}

/// What the folder of an Actor's Agent de support remembers: the agent open
/// now (`None` once closed), and the last choice the Actor opened one with,
/// which the next opening proposes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupportRecord {
    #[serde(default)]
    pub agent: Option<SupportAgent>,
    #[serde(default)]
    pub last_choice: Option<AgentChoice>,
}

/// The working folder of the Agent de support `session`, under `root` (the
/// daemon's state folder): its cwd, its primer, its record.
pub fn support_dir(root: &Path, session: &str) -> PathBuf {
    root.join(session)
}

/// The primer of the agent working in `dir`.
pub fn support_primer_path(dir: &Path) -> PathBuf {
    dir.join("support-agent.md")
}

/// The record of the agent working in `dir`.
pub fn support_record_path(dir: &Path) -> PathBuf {
    dir.join(".support.json")
}

/// What the agent reads first: `prompt` (the embedded one), then the person's
/// first `question`, or, without one, the instruction to wait for it.
pub fn support_primer(prompt: &str, question: Option<&str>) -> String {
    let opening = match question.map(str::trim).filter(|q| !q.is_empty()) {
        Some(question) => format!("## The person's first question\n\n{question}\n"),
        None => "## Start\n\nGreet the person in one short line, then wait for their question.\n"
            .to_string(),
    };
    format!("{}\n\n{opening}", prompt.trim_end())
}

/// A question as it is typed into a live agent: trimmed; `None` when empty.
pub fn question_text(question: &str) -> Option<&str> {
    Some(question.trim()).filter(|q| !q.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_is_embedded_and_short() {
        assert!(SUPPORT_AGENT_PROMPT.contains("support agent"));
        assert!(SUPPORT_AGENT_PROMPT.contains("pdo help"));
        assert!(SUPPORT_AGENT_PROMPT.contains("pdo ext <id> help"));
        // One page at most.
        assert!(SUPPORT_AGENT_PROMPT.lines().count() <= 60);
    }

    #[test]
    fn the_first_question_closes_the_primer() {
        let primer = support_primer("ROLE\n", Some("  which Runs failed?  "));
        assert!(primer.starts_with("ROLE\n\n"), "{primer}");
        assert!(primer.ends_with("## The person's first question\n\nwhich Runs failed?\n"));
        let waiting = support_primer("ROLE", Some("   "));
        assert!(waiting.contains("wait for their question"), "{waiting}");
        assert!(support_primer("ROLE", None).contains("## Start"));
    }

    #[test]
    fn the_folder_holds_the_primer_and_the_record() {
        let dir = support_dir(Path::new("/state/support"), "pdo-support-operator");
        assert_eq!(dir, Path::new("/state/support/pdo-support-operator"));
        assert_eq!(support_primer_path(&dir), dir.join("support-agent.md"));
        assert_eq!(support_record_path(&dir), dir.join(".support.json"));
    }

    #[test]
    fn an_empty_question_is_none() {
        assert_eq!(question_text(" hi \n"), Some("hi"));
        assert_eq!(question_text(" \n "), None);
    }

    #[test]
    fn a_record_without_fields_reads_as_empty() {
        let record: SupportRecord = serde_json::from_str("{}").unwrap();
        assert_eq!(record, SupportRecord::default());
    }
}
