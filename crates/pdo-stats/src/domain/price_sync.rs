//! The price sync (ADR-0034): what the price source's payload becomes, and
//! what a sync changes in the table in force. The table itself — its tiers,
//! their parsing and their read from disk — is the run context's
//! (`pdo_run::domain::prices`), which prices a Run's cost.

use std::collections::BTreeMap;
use std::time::Duration;

use pdo_run::domain::prices::parse::validate_price;
use pdo_run::domain::prices::{strip_date_suffix, Price, PriceTable, PriceTier, RejectedRow};

/// Default price source (ADR-0034): models.dev, `anthropic` provider only.
pub const PRICE_SOURCE_URL_DEFAULT: &str = "https://models.dev/api.json";

/// Hard ceiling on one fetch. 10 s, not the 3 s of `DOCKER_PROBE_TIMEOUT`: the
/// payload is ~3.3 MB.
pub const PRICE_FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// Guard against a hostile or aberrant body.
pub const PRICE_FETCH_MAX_BYTES: usize = 16 * 1024 * 1024;

/// Age past which the boot refresh re-fetches an EXISTING cache. It never
/// creates one (ADR-0034: no egress before the first explicit click).
pub const PRICE_REFRESH_MAX_AGE: Duration = Duration::from_secs(24 * 3600);

/// Normalise a models.dev payload into rows ready to write. PURE.
///
/// Reads ONLY `root["anthropic"]["models"]`. The other 174 providers carry
/// re-prefixed ids (`anthropic.claude-opus-5`, `us.anthropic.…`) and REGIONAL
/// prices (`eu.anthropic.claude-opus-5` is +10 %) — mixing them would reintroduce
/// exactly the collisions that disqualified OpenRouter.
///
/// - keeps ids matching `claude-`; everything else is ignored WITHOUT error
///   (noise, not a defect)
/// - de-dates the key, and never strips `-fast` (stripping creates a
///   $5/$25-vs-$10/$50 collision; keeping it yields a key no transcript matches,
///   which costs nothing)
/// - takes `cost.input` / `cost.output` in **$/MTok as-is** — models.dev is
///   already in the right unit, which is the other reason to prefer it: there is
///   no 10⁶ factor to get wrong
/// - a divergent-price collision after de-dating DROPS THE WHOLE KEY, named
///   (a source defect, not something to arbitrate by heuristic — the posture of
///   #395's "never a false `synced` verdict")
/// - an EMPTY harvest is an `Err`: an upstream schema drift would otherwise write
///   an empty `fetched.json` and destroy the last known table
#[allow(clippy::type_complexity)]
pub fn normalize_models_dev(
    text: &str,
) -> Result<(BTreeMap<String, Price>, Vec<RejectedRow>), String> {
    let root: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("response is not valid JSON: {e}"))?;
    let models = root
        .get("anthropic")
        .and_then(|p| p.get("models"))
        .and_then(|m| m.as_object())
        .ok_or_else(|| {
            "no `anthropic.models` object in the response — the source schema has drifted"
                .to_string()
        })?;

    let mut candidates: BTreeMap<String, Vec<Price>> = BTreeMap::new();
    let mut rejected: Vec<RejectedRow> = Vec::new();

    for (map_key, entry) in models {
        let id = entry
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or(map_key.as_str());
        if !id.starts_with("claude-") {
            continue; // noise, not a defect
        }
        let key = strip_date_suffix(id).to_string();
        let cost = entry.get("cost");
        let num = |field: &str| -> Result<f64, String> {
            cost.and_then(|c| c.get(field))
                .ok_or_else(|| format!("missing `cost.{field}`"))?
                .as_f64()
                .ok_or_else(|| format!("`cost.{field}` is not a number"))
        };
        match num("input").and_then(|i| num("output").and_then(|o| validate_price(i, o))) {
            Ok(price) => candidates.entry(key).or_default().push(price),
            Err(why) => rejected.push(RejectedRow { key, why }),
        }
    }

    let mut rows: BTreeMap<String, Price> = BTreeMap::new();
    for (key, prices) in candidates {
        let first = prices[0];
        if prices.iter().all(|p| *p == first) {
            rows.insert(key, first);
        } else {
            let seen = prices
                .iter()
                .map(|p| format!("${}/${}", p.input, p.output))
                .collect::<Vec<_>>()
                .join(" vs ");
            rejected.push(RejectedRow {
                key,
                why: format!(
                    "de-dating collapses several source ids onto this key at DIVERGENT prices \
                     ({seen}) — the whole key is dropped rather than arbitrated"
                ),
            });
        }
    }

    if rows.is_empty() {
        return Err(
            "zero usable `claude-*` row in the response — refusing to write, so the last known \
             table survives (ADR-0034)"
                .to_string(),
        );
    }
    Ok((rows, rejected))
}

/// testable without an HTTP round-trip.
pub struct PriceSyncOutcome {
    pub rows: usize,
    pub added: Vec<String>,
    pub updated: Vec<String>,
    pub unchanged: usize,
    pub shadowed_by_manual: Vec<String>,
    pub changed: bool,
}

/// Diff a freshly normalised fetched tier against the table currently in force.
/// PURE. `changed` is keyed on the RAW fetched rows, NOT on effective prices: a
/// shadowed key whose source price moved is a real change to the fetched tier even
/// though nothing a user sees moves.
pub fn diff_price_sync(
    before: &PriceTable,
    fresh: &std::collections::BTreeMap<String, Price>,
) -> PriceSyncOutcome {
    let mut out = PriceSyncOutcome {
        rows: fresh.len(),
        added: Vec::new(),
        updated: Vec::new(),
        unchanged: 0,
        shadowed_by_manual: Vec::new(),
        changed: fresh != before.fetched_prices(),
    };
    for (key, fresh_price) in fresh {
        let shadowed = before.tier_of(key) == Some(PriceTier::Manual);
        if shadowed {
            out.shadowed_by_manual.push(key.clone());
        }
        let effective_before = before.price_for(key);
        let effective_after = if shadowed {
            effective_before
        } else {
            Some(*fresh_price)
        };
        match effective_before {
            None => out.added.push(key.clone()),
            Some(_) if effective_before != effective_after => out.updated.push(key.clone()),
            Some(_) => out.unchanged += 1,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(input: f64, output: f64) -> Price {
        Price { input, output }
    }

    // --- normalize_models_dev ---

    /// The real shape of `models.dev/api.json`, trimmed to what we read.
    const MODELS_DEV: &str = r#"{
      "anthropic": { "models": {
        "claude-opus-5":   { "id": "claude-opus-5",   "cost": { "input": 5,  "output": 25, "cache_read": 0.5, "cache_write": 6.25 } },
        "claude-sonnet-5": { "id": "claude-sonnet-5", "cost": { "input": 2,  "output": 10 } },
        "claude-fable-5":  { "id": "claude-fable-5",  "cost": { "input": 10, "output": 50 } },
        "claude-haiku-4-5-20251001": { "id": "claude-haiku-4-5-20251001", "cost": { "input": 1, "output": 5 } },
        "claude-opus-5-fast": { "id": "claude-opus-5-fast", "cost": { "input": 10, "output": 50 } },
        "gpt-nope": { "id": "gpt-nope", "cost": { "input": 1, "output": 1 } }
      } },
      "eu.anthropic": { "models": {
        "eu.anthropic.claude-opus-5": { "id": "claude-opus-5", "cost": { "input": 5.5, "output": 27.5 } }
      } }
    }"#;

    #[test]
    fn normalize_reads_the_real_shape_in_dollars_per_mtok() {
        let (rows, rejected) = normalize_models_dev(MODELS_DEV).unwrap();
        // The test that fails if anyone multiplies or divides by 10^6.
        assert_eq!(rows.get("claude-opus-5"), Some(&p(5.0, 25.0)));
        assert_eq!(rows.get("claude-sonnet-5"), Some(&p(2.0, 10.0)));
        assert_eq!(rows.get("claude-fable-5"), Some(&p(10.0, 50.0)));
        assert!(rejected.is_empty(), "rejected = {rejected:?}");
    }

    #[test]
    fn normalize_de_dates_the_key() {
        let (rows, _) = normalize_models_dev(MODELS_DEV).unwrap();
        assert_eq!(
            rows.get("claude-haiku-4-5"),
            Some(&p(1.0, 5.0)),
            "a dated source id must price the family the transcripts write"
        );
        assert!(!rows.contains_key("claude-haiku-4-5-20251001"));
    }

    #[test]
    fn normalize_never_strips_fast() {
        let (rows, _) = normalize_models_dev(MODELS_DEV).unwrap();
        // Stripping would collide with claude-opus-5 at a different price.
        assert_eq!(rows.get("claude-opus-5-fast"), Some(&p(10.0, 50.0)));
        assert_eq!(rows.get("claude-opus-5"), Some(&p(5.0, 25.0)));
    }

    #[test]
    fn normalize_ignores_other_providers_and_non_claude_ids() {
        let (rows, _) = normalize_models_dev(MODELS_DEV).unwrap();
        // Regional prices (+10 %) would reintroduce OpenRouter's collisions.
        assert_eq!(rows.get("claude-opus-5"), Some(&p(5.0, 25.0)));
        assert!(!rows.keys().any(|k| k.contains("eu.")));
        assert!(!rows.contains_key("gpt-nope"));
    }

    #[test]
    fn normalize_drops_a_whole_key_on_a_divergent_collision() {
        let text = r#"{"anthropic":{"models":{
            "claude-zed-1-20260101": { "id": "claude-zed-1-20260101", "cost": { "input": 1, "output": 2 } },
            "claude-zed-1-20260202": { "id": "claude-zed-1-20260202", "cost": { "input": 9, "output": 9 } },
            "claude-opus-5": { "id": "claude-opus-5", "cost": { "input": 5, "output": 25 } }
        }}}"#;
        let (rows, rejected) = normalize_models_dev(text).unwrap();
        assert!(
            !rows.contains_key("claude-zed-1"),
            "a divergent collision drops the whole key rather than arbitrating"
        );
        assert!(rejected.iter().any(|r| r.key == "claude-zed-1"));
        assert!(rows.contains_key("claude-opus-5"), "the other keys survive");
    }

    #[test]
    fn normalize_accepts_an_identical_price_collision() {
        let text = r#"{"anthropic":{"models":{
            "claude-zed-1-20260101": { "id": "claude-zed-1-20260101", "cost": { "input": 1, "output": 2 } },
            "claude-zed-1-20260202": { "id": "claude-zed-1-20260202", "cost": { "input": 1, "output": 2 } }
        }}}"#;
        let (rows, rejected) = normalize_models_dev(text).unwrap();
        assert_eq!(rows.get("claude-zed-1"), Some(&p(1.0, 2.0)));
        assert!(rejected.is_empty());
    }

    #[test]
    fn normalize_errors_on_an_empty_harvest_or_a_drifted_schema() {
        // The guard that stops a schema drift from destroying the last table.
        assert!(normalize_models_dev("{}").is_err());
        assert!(normalize_models_dev(r#"{"anthropic":{"models":{}}}"#).is_err());
        assert!(normalize_models_dev(
            r#"{"anthropic":{"models":{"gpt-1":{"cost":{"input":1,"output":1}}}}}"#
        )
        .is_err());
        assert!(normalize_models_dev("{not json").is_err());
    }

    #[test]
    fn normalize_rejects_a_row_with_a_bad_cost_but_keeps_the_others() {
        let text = r#"{"anthropic":{"models":{
            "claude-bad-1": { "id": "claude-bad-1", "cost": { "input": "5", "output": 25 } },
            "claude-opus-5": { "id": "claude-opus-5", "cost": { "input": 5, "output": 25 } }
        }}}"#;
        let (rows, rejected) = normalize_models_dev(text).unwrap();
        assert!(!rows.contains_key("claude-bad-1"));
        assert!(rows.contains_key("claude-opus-5"));
        assert_eq!(rejected.len(), 1);
    }

    #[test]
    fn a_sync_diff_keys_changed_on_the_raw_rows_and_names_the_shadowed_keys() {
        let before = PriceTable::resolve(
            pdo_run::domain::prices::parse::parse_manual(
                "schema: pdo.prices/v1\nmodels:\n  claude-opus-5: { input: 1, output: 2 }\n",
            ),
            Default::default(),
            0,
        );
        let fresh: BTreeMap<String, Price> = [
            ("claude-opus-5".to_string(), p(5.0, 25.0)),
            ("claude-new-1".to_string(), p(3.0, 4.0)),
        ]
        .into_iter()
        .collect();
        let outcome = diff_price_sync(&before, &fresh);
        assert!(outcome.changed);
        assert_eq!(outcome.rows, 2);
        assert_eq!(outcome.shadowed_by_manual, ["claude-opus-5"]);
        assert_eq!(outcome.added, ["claude-new-1"]);
    }
}
