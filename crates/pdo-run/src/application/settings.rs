//! The instance settings a Run reads, fresh at every seam that needs them, so a
//! `PUT /settings` takes effect on the next spawn or sweep with no restart
//! (ADR-0015, `stored → env → default`). An unreadable store degrades each
//! tier to transparent rather than failing the caller.

use std::collections::BTreeMap;

use crate::application::stale::detector::autocomplete_turn_end_with;
use crate::Runs;

/// The stored session cap, before the environment tier.
pub async fn stored_session_cap(runs: &Runs) -> Option<usize> {
    runs.settings
        .defaults()
        .await
        .and_then(|c| c.session_cap)
        .map(|n| n as usize)
}

/// The instance-wide default model, resolved `stored → env → None`.
pub async fn stored_default_model(runs: &Runs) -> Option<String> {
    let stored = runs.settings.defaults().await.and_then(|c| c.default_model);
    pdo_session::adapters::env::default_model_with(stored)
}

/// The instance-wide default **harness**, the `instance` tier of the harness
/// resolver. `None` ⇒ the resolver's `claude` floor.
pub async fn stored_default_harness(runs: &Runs) -> Option<String> {
    let stored = runs
        .settings
        .defaults()
        .await
        .and_then(|c| c.default_harness);
    pdo_session::adapters::env::default_harness_with(stored)
}

/// The per-harness default model map for the resolver. The legacy single
/// instance default (`default_model`) is folded under `claude` when the map is
/// silent for it, so an older setup keeps working unchanged.
pub async fn stored_default_harness_models(runs: &Runs) -> BTreeMap<String, String> {
    let cfg = runs.settings.defaults().await;
    let mut map = cfg
        .as_ref()
        .map(|c| c.default_harness_model.clone())
        .unwrap_or_default();
    if !map.contains_key(pdo_harness::registry::CLAUDE) {
        if let Some(legacy) =
            pdo_session::adapters::env::default_model_with(cfg.and_then(|c| c.default_model))
        {
            map.insert(pdo_harness::registry::CLAUDE.to_string(), legacy);
        }
    }
    map
}

/// Turn-end auto-completion, resolved the way the liveness sweep does. The hook
/// and the sweep are one policy on ONE setting (ADR-0043) — there is
/// deliberately no second toggle.
pub async fn stored_autocomplete_turn_end(runs: &Runs) -> bool {
    let stored = runs
        .settings
        .defaults()
        .await
        .and_then(|c| c.autocomplete_turn_end);
    autocomplete_turn_end_with(stored)
}
