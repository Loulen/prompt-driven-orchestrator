//! The price sync (ADR-0034): the single egress that GETs the price source,
//! its normalisation into rows, and the atomic write of the fetched tier. The
//! table itself — its tiers, their parsing and their read from disk — is the
//! run context's (`pdo_run::domain::prices`), which prices a Run's cost.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub(crate) use pdo_run::domain::prices::parse::{validate_price, FETCHED_SCHEMA};
pub(crate) use pdo_run::domain::prices::{strip_date_suffix, Price, PriceTable, PriceTier, RejectedRow};

/// Default price source (ADR-0034): models.dev, `anthropic` provider only.
pub(crate) const PRICE_SOURCE_URL_DEFAULT: &str = "https://models.dev/api.json";

/// Hard ceiling on one fetch. 10 s, not the 3 s of `DOCKER_PROBE_TIMEOUT`: the
/// payload is ~3.3 MB.
pub(crate) const PRICE_FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// Guard against a hostile or aberrant body.
pub(crate) const PRICE_FETCH_MAX_BYTES: usize = 16 * 1024 * 1024;

/// Age past which the boot refresh re-fetches an EXISTING cache. It never
/// creates one (ADR-0034: no egress before the first explicit click).
pub(crate) const PRICE_REFRESH_MAX_AGE: Duration = Duration::from_secs(24 * 3600);


/// GET the price source — the single egress, strictly outside the read path
/// (ADR-0034). The daemon's ONLY outbound call outside `docker pull` and the
/// shelled Trigger guards.
///
/// **Async, mandatorily**: `reqwest::blocking` panics when invoked from inside
/// the runtime context, including from a `spawn_blocking` thread (those carry the
/// context) — `main.rs:18-20` documents this for the CLI paths. This is the
/// crate's first async reqwest call, so there is no precedent to copy.
pub(crate) async fn fetch_source(url: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(PRICE_FETCH_TIMEOUT)
        .connect_timeout(PRICE_FETCH_TIMEOUT)
        .build()
        .map_err(|e| format!("cannot build the HTTP client: {e}"))?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("source answered HTTP {status}"));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("cannot read the body: {e}"))?;
    if bytes.len() > PRICE_FETCH_MAX_BYTES {
        return Err(format!(
            "body is {} bytes, over the {PRICE_FETCH_MAX_BYTES}-byte ceiling",
            bytes.len()
        ));
    }
    String::from_utf8(bytes.to_vec()).map_err(|e| format!("body is not UTF-8: {e}"))
}

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
pub(crate) fn normalize_models_dev(
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

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Serialise and write `fetched.json` by tmp + `rename` **in the same
/// directory** (idiom of the sandbox staging's `atomic_copy_into`), so a concurrent
/// `PriceTable::load` can never read a half-written document.
///
/// **Never writes an empty table** — the one path by which this feature could
/// destroy something.
pub(crate) fn write_fetched(
    path: &Path,
    source: &str,
    fetched_at: &str,
    rows: &BTreeMap<String, Price>,
) -> std::io::Result<()> {
    if rows.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "refusing to write an empty price table (ADR-0034)",
        ));
    }
    let dir = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent")
    })?;
    std::fs::create_dir_all(dir)?;
    let doc = serde_json::json!({
        "schema": FETCHED_SCHEMA,
        "source": source,
        "fetched_at": fetched_at,
        "models": rows,
    });
    let body = serde_json::to_vec_pretty(&doc)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!("fetched.json.pdo-tmp.{}.{seq}", std::process::id()));
    std::fs::write(&tmp, &body)?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp); // never leave an orphan
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdo_run::domain::prices::parse::parse_fetched;

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

    // --- write_fetched ---

    #[test]
    fn write_fetched_round_trips_through_parse_fetched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prices").join("fetched.json");
        let rows: BTreeMap<String, Price> = [
            ("claude-fable-5".to_string(), p(10.0, 50.0)),
            ("claude-opus-5".to_string(), p(5.0, 25.0)),
        ]
        .into_iter()
        .collect();
        write_fetched(
            &path,
            "https://models.dev/api.json",
            "2026-07-30T14:12:03Z",
            &rows,
        )
        .unwrap();

        let tier = parse_fetched(&std::fs::read_to_string(&path).unwrap());
        assert!(tier.unparseable.is_none());
        assert_eq!(tier.rows, rows);
        assert_eq!(
            tier.provenance.map(|x| x.source),
            Some("https://models.dev/api.json".to_string())
        );
        // No temp file left behind.
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n != "fetched.json")
            .collect();
        assert!(leftovers.is_empty(), "leftovers = {leftovers:?}");
    }

    #[test]
    fn write_fetched_refuses_an_empty_table() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fetched.json");
        let err = write_fetched(&path, "s", "t", &BTreeMap::new()).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(!path.exists(), "nothing may be written");
    }

    #[test]
    fn write_fetched_leaves_the_previous_table_intact_on_a_refusal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fetched.json");
        let rows: BTreeMap<String, Price> = [("claude-opus-5".to_string(), p(5.0, 25.0))]
            .into_iter()
            .collect();
        write_fetched(&path, "s", "t1", &rows).unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(write_fetched(&path, "s", "t2", &BTreeMap::new()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before, "byte for byte");
    }
}

