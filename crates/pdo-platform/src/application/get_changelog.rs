//! « What's new » (#698): the release notes of every version strictly newer than
//! the installed one, newest first. The release list is fetched on demand and
//! memoised per latest version; the payload says where its body comes from:
//!
//! - `source: "releases"` — assembled from the missed releases;
//! - `source: "embedded"`, `fallback_reason: null` — up to date, the embedded
//!   `CHANGELOG.md` is the nominal body;
//! - `source: "embedded"`, `fallback_reason: "…"` — the list was NOT available
//!   (check off, source unreachable at last check, list fetch failed).
//!
//! Never an error to the caller: the embedded changelog always answers.

use std::sync::Arc;

use super::update_context::UpdateContext;
use crate::domain::changelog::{assemble_changelog, embedded_changelog, Changelog};
use crate::domain::release::{is_newer, releases_list_url};

pub struct GetChangelog {
    update: Arc<UpdateContext>,
}

impl GetChangelog {
    pub fn new(update: Arc<UpdateContext>) -> Self {
        Self { update }
    }

    pub async fn execute(&self) -> Changelog {
        self.update.ensure_loaded().await;
        let installed = self.update.channel().installed_version.as_str();
        let enabled = self.update.ports.setting.enabled().await;
        let cache = self.update.cached().await;

        let Some(source_url) = self.update.source_url() else {
            return embedded_changelog(
                installed,
                None,
                "No release source is declared for this build.",
            );
        };
        if !enabled {
            return embedded_changelog(installed, None, "The update check is off.");
        }
        let Some(latest) = cache.as_ref().and_then(|c| c.latest_version.clone()) else {
            let reason = match cache.as_ref().and_then(|c| c.error.as_deref()) {
                Some(_) => "The release source was unreachable at the last check.",
                None => "The release source has not been checked yet.",
            };
            return embedded_changelog(installed, None, reason);
        };
        if !is_newer(installed, &latest) {
            // Up to date: nothing to assemble, the embedded changelog is the body.
            return assemble_changelog(installed, Some(&latest), &[]);
        }

        let memo = self.update.releases.lock().await.clone();
        let releases = match memo {
            Some((for_latest, rels)) if for_latest == latest => Ok(rels),
            _ => {
                let url = releases_list_url(&source_url);
                let fetched = self.update.ports.source.releases(&url).await;
                if let Ok(rels) = &fetched {
                    *self.update.releases.lock().await = Some((latest.clone(), rels.clone()));
                }
                fetched.map_err(|e| format!("release list unavailable: {url}: {e}"))
            }
        };
        match releases {
            Ok(rels) => assemble_changelog(installed, Some(&latest), &rels),
            Err(e) => {
                tracing::warn!("update changelog: {e} — falling back to the embedded changelog");
                embedded_changelog(
                    installed,
                    Some(&latest),
                    "The release source was unreachable when loading the release notes.",
                )
            }
        }
    }
}
