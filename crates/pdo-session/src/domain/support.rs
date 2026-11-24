//! The Agent de support: an interactive agent the core holds outside any Run,
//! one per Actor, with the rights of its Actor. What it reads (its
//! instructions, the embedded prompt, in the files its harness loads from its
//! working folder; then the person's first question as its first message),
//! where its folder is, and what that folder remembers across a restart of the
//! daemon.

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
/// daemon's state folder): its cwd, its instructions, its record.
pub fn support_dir(root: &Path, session: &str) -> PathBuf {
    root.join(session)
}

/// The instructions of the agent working in `dir`: `AGENTS.md`, the file the
/// harnesses load from their working folder at start, so the embedded prompt
/// is never typed into the conversation.
pub fn support_instructions_path(dir: &Path) -> PathBuf {
    dir.join("AGENTS.md")
}

/// The file claude loads from its working folder instead of `AGENTS.md`; it
/// holds [`SUPPORT_INSTRUCTIONS_IMPORT`].
pub fn support_instructions_import_path(dir: &Path) -> PathBuf {
    dir.join("CLAUDE.md")
}

/// What [`support_instructions_import_path`] holds: an import of `AGENTS.md`,
/// so the instructions are written once.
pub const SUPPORT_INSTRUCTIONS_IMPORT: &str = "@AGENTS.md\n";

/// The first question of the agent working in `dir`, its first message.
pub fn support_question_path(dir: &Path) -> PathBuf {
    dir.join("question.md")
}

/// The record of the agent working in `dir`.
pub fn support_record_path(dir: &Path) -> PathBuf {
    dir.join(".support.json")
}

/// The first message of an agent asked `question`, as its file holds it: one
/// that starts with `-` gets a space in front, so the harness never reads it as
/// one of its options (`--help` would print its help and end the session).
pub fn first_message(question: &str) -> String {
    let lead = if question.starts_with('-') { " " } else { "" };
    format!("{lead}{question}\n")
}

/// The primer an earlier version wrote in the folder; nothing reads it now.
pub const SUPERSEDED_PRIMER: &str = "support-agent.md";

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
    fn the_folder_holds_the_instructions_the_question_and_the_record() {
        let dir = support_dir(Path::new("/state/support"), "pdo-support-operator");
        assert_eq!(dir, Path::new("/state/support/pdo-support-operator"));
        assert_eq!(support_instructions_path(&dir), dir.join("AGENTS.md"));
        assert_eq!(
            support_instructions_import_path(&dir),
            dir.join("CLAUDE.md")
        );
        assert_eq!(SUPPORT_INSTRUCTIONS_IMPORT, "@AGENTS.md\n");
        assert_eq!(support_question_path(&dir), dir.join("question.md"));
        assert_eq!(support_record_path(&dir), dir.join(".support.json"));
    }

    #[test]
    fn a_first_message_is_never_read_as_an_option() {
        assert_eq!(first_message("which Runs failed?"), "which Runs failed?\n");
        assert_eq!(first_message("--help me"), " --help me\n");
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
