//! [`SessionTranscripts`] over the harnesses' own files, resolved by the
//! harness context's transcript layouts.

use std::path::{Path, PathBuf};

use pdo_harness::adapters::transcripts::{resolve_transcript, subagent_transcripts};
use pdo_harness::claude_transcript::cc_project_dirname;

use crate::application::ports::SessionTranscripts;

/// The harnesses' transcripts on this host's disk.
pub struct HarnessTranscripts;

impl SessionTranscripts for HarnessTranscripts {
    fn source_listable(&self, root: &Path) -> Result<(), String> {
        if !root.exists() {
            return Ok(());
        }
        std::fs::read_dir(root)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn main_session(
        &self,
        harness: &str,
        root: &Path,
        working_dir: &Path,
        session_id: Option<&str>,
    ) -> Option<String> {
        resolve_transcript(harness, root, working_dir, session_id)
            .and_then(|path| std::fs::read_to_string(path).ok())
    }

    fn subagents(
        &self,
        harness: &str,
        root: &Path,
        working_dir: &Path,
        session_id: &str,
    ) -> Vec<(String, String)> {
        subagent_transcripts(harness, root, working_dir, session_id)
    }

    fn claude_sessions_in(
        &self,
        claude_root: &Path,
        working_dir: &Path,
    ) -> Option<(PathBuf, Vec<String>)> {
        let project_dir = claude_root.join(cc_project_dirname(working_dir));
        let entries = std::fs::read_dir(&project_dir).ok()?;
        let mut sessions = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            sessions.push(stem.to_string());
        }
        Some((project_dir, sessions))
    }

    fn read(&self, path: &Path) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }
}
