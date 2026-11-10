//! Scan a Source (#670): clone it shallow (or open a local folder), walk it for
//! `SKILL.md`s, validate each, and report the collisions with the bank.
//! Nothing is written to the bank; the source joins the recent ones.

use std::path::Path;

use pdo_kernel::clock::now_iso;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::ports::ScannedSource;
use super::skills::Skills;
use crate::domain::import::{classify, ScanOutcome, ScannedCandidate};
use crate::domain::import_error::ImportError;
use crate::domain::source::{parse_source, ParsedSource};

/// A scan, its candidates against the bank.
#[derive(Debug, Clone)]
pub struct ScanReport {
    pub outcome: ScanOutcome,
    pub candidates: Vec<ScannedCandidate>,
}

impl Skills {
    /// `home` expands a leading `~` in `source`. The clone is kept under the
    /// client-chosen `scan_id` for the import that follows.
    pub async fn scan_source(
        &self,
        actor: &Actor,
        scan_id: &str,
        source: &str,
        home: Option<&Path>,
    ) -> Result<ScanReport, ImportError> {
        let source = parse_source(source, home)?;
        self.authorize(
            actor,
            Operation::new(ResourceKind::Skill, Verb::Action("scan".into())),
        )
        .map_err(|denied| ImportError::Skill(denied.into()))?;
        let mut outcome = self.scanner.scan(scan_id, &source).await?;
        let folders = self.folders.list().await?;
        let mut candidates = Vec::with_capacity(outcome.candidates.len());
        for candidate in std::mem::take(&mut outcome.candidates) {
            let existing = if candidate.valid {
                self.skills.find_by_name(&candidate.name).await?
            } else {
                None
            };
            candidates.push(classify(
                candidate,
                existing.as_ref(),
                &outcome.source,
                outcome.commit.as_deref(),
                &folders,
            ));
        }
        self.remember(&outcome.source).await;
        Ok(ScanReport {
            outcome,
            candidates,
        })
    }

    /// Kill the clone running under `scan_id` (the client also aborted its
    /// request). `true` when something was running under that id.
    pub fn cancel_scan(&self, actor: &Actor, scan_id: &str) -> Result<bool, ImportError> {
        self.authorize(
            actor,
            Operation::new(ResourceKind::Skill, Verb::Action("cancel_scan".into())),
        )
        .map_err(|denied| ImportError::Skill(denied.into()))?;
        Ok(self.scanner.cancel(scan_id))
    }

    /// Remember a scanned source for the popup's "Recent sources" list. Never
    /// fails the gesture that scanned it.
    pub(crate) async fn remember(&self, source: &ParsedSource) {
        let _ = self
            .sources
            .remember(&ScannedSource {
                url: source.url.clone(),
                git_ref: source.git_ref.clone().unwrap_or_default(),
                path: source.path.clone(),
                last_used_at: now_iso(),
            })
            .await;
    }
}
