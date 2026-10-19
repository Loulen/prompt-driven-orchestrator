//! `GET /settings → harness_descriptors` (#553, #586, #616, #705, #798).

use std::path::Path;

use serde_json::{json, Value};

use crate::application::harness_offers::HarnessOffer;
use crate::domain::catalogue::CachedCatalogue;
use crate::domain::registry::HarnessRegistry;

/// The `harness_descriptors` block: where the descriptor file lives, what the
/// registry resolves, each harness's offer, and what the disk tier refused. The
/// path is ALWAYS reported so the user knows where to write — nothing is ever
/// seeded.
pub fn descriptors_view(
    home_root: Option<&Path>,
    registry: &HarnessRegistry,
    offers: &[HarnessOffer],
) -> Value {
    let harness_list: Vec<Value> = offers
        .iter()
        .map(|o| {
            offer_entry(
                &o.name,
                o.source.as_str(),
                o.installed,
                &o.cached,
                o.has_effort,
            )
        })
        .collect();
    match home_root {
        Some(home) => json!({
            "path": HarnessRegistry::descriptors_path(home).to_string_lossy(),
            "names": registry.names(),
            "harnesses": harness_list,
            "rejected": registry
                .rejected()
                .iter()
                .map(|r| json!({ "name": r.name, "why": r.why }))
                .collect::<Vec<_>>(),
            "reason": registry.diagnostic(),
        }),
        None => json!({
            "path": null,
            "names": registry.names(),
            "harnesses": harness_list,
            "rejected": [],
            "reason": "HOME is unset, so the descriptor file has no resolvable path — \
                       only the harnesses compiled into the binary apply",
        }),
    }
}

/// One harness's picker entry of `harness_descriptors.harnesses` (#586, #616,
/// #705, #798). PURE — the probed facts in, the wire JSON out — so the shape is
/// pinned by a test without an installed binary.
pub fn offer_entry(
    name: &str,
    source: &str,
    installed: bool,
    cached: &CachedCatalogue,
    has_effort: bool,
) -> Value {
    json!({
        "name": name,
        "source": source,
        "installed": installed,
        "version": cached.version,
        "models": cached.catalogue.models,
        // #705: the context window a source published beside each id (`pi
        // --list-models`), keyed by id — a picker hint, verbatim. Empty when no
        // source names one.
        "model_contexts": cached.catalogue.model_contexts,
        // #798: the per-model effort table, keyed by id like `model_contexts`. A
        // MISSING key is unknown — the client falls back to the global `efforts`
        // axis; a PRESENT key (an empty vec included) is the model's known offer,
        // verbatim from the binary's RPC report.
        "model_efforts": cached.catalogue.model_efforts,
        "efforts": cached.catalogue.efforts,
        "has_effort": has_effort,
        // When the catalogue ladder last ran: the « probed … » line of the
        // pickers and of Settings › Diagnostics. `null` when never probed.
        "probed_at": cached.probed_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #798: the picker entry serves the per-model effort table beside
    /// `model_contexts`, verbatim — and the semantics it carries are the wire
    /// contract: an absent key would be unknown (global fallback), a present one
    /// (here, `[off]` included) is the model's known offer.
    #[test]
    fn offer_entry_serves_the_per_model_effort_table() {
        let mut catalogue = crate::domain::catalogue::Catalogue {
            models: vec![
                "openrouter/openai/gpt-5.4".to_string(),
                "openrouter/stealth/union-alpha".to_string(),
            ],
            efforts: vec![
                "off".to_string(),
                "minimal".to_string(),
                "low".to_string(),
                "medium".to_string(),
                "high".to_string(),
                "xhigh".to_string(),
                "max".to_string(),
            ],
            ..Default::default()
        };
        catalogue
            .model_contexts
            .insert("openrouter/openai/gpt-5.4".to_string(), "400K".to_string());
        catalogue.model_efforts.insert(
            "openrouter/openai/gpt-5.4".to_string(),
            vec![
                "off".to_string(),
                "low".to_string(),
                "medium".to_string(),
                "high".to_string(),
                "xhigh".to_string(),
            ],
        );
        catalogue.model_efforts.insert(
            "openrouter/stealth/union-alpha".to_string(),
            vec!["off".to_string()],
        );
        let entry = offer_entry(
            "pi",
            "builtin",
            true,
            &CachedCatalogue {
                version: Some("pi 0.85.1".to_string()),
                catalogue,
                probed_at: Some("2026-10-19T08:00:00.000Z".to_string()),
            },
            true,
        );

        assert_eq!(entry["name"], "pi");
        assert_eq!(entry["version"], "pi 0.85.1");
        // The exact levels the binary reported, beside the context window, keyed
        // by the same ids as `models`.
        assert_eq!(
            entry["model_efforts"]["openrouter/openai/gpt-5.4"],
            serde_json::json!(["off", "low", "medium", "high", "xhigh"])
        );
        assert_eq!(
            entry["model_efforts"]["openrouter/stealth/union-alpha"],
            serde_json::json!(["off"])
        );
        assert_eq!(entry["model_contexts"]["openrouter/openai/gpt-5.4"], "400K");
        assert_eq!(entry["efforts"].as_array().map(Vec::len), Some(7));
        assert_eq!(entry["probed_at"], "2026-10-19T08:00:00.000Z");
    }
}
