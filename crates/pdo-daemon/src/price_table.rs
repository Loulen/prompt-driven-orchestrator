//! The price table in force for ONE cost read: three tiers merged **by family
//! key** — manual (`~/.pdo/prices/models.yaml`, written by the human alone) →
//! fetched (`~/.pdo/prices/fetched.json`, written by the daemon alone) →
//! embedded ([`PRICES`], the compiled floor). See ADR-0034 and ADR-0022's #427
//! amendment.
//!
//! This module never reads `$HOME`: a root goes in, path-math + `std::fs` come
//! out (the discipline #408 paid a slice to give `run_cost` — `library_store`'s
//! global `$HOME` read costs a crate-wide test lock, `library_store.rs:967`).
//!
//! ## What is load-bearing here
//! - **Merge by key, never replacement.** A key present in a tier wins; a key
//!   absent keeps whatever the next tier says. Under global replacement,
//!   forgetting `claude-opus-4-8` would erase 79 941 of ~116 200 transcript
//!   lines — a wrong-price bug converted into a total blackout — and would
//!   freeze the table against future releases.
//! - **The embedded tier is a FLOOR, not a seed.** `claude-opus-4-0`,
//!   `claude-sonnet-4-0` and `claude-3-5-haiku` are absent from every remote
//!   source examined, so the `const` is their ONLY pricer.
//! - **De-dating is asymmetric on purpose.** A dated key in the *manual* file is
//!   REFUSED (stripping would silently collapse two rows the author wanted
//!   distinct, and the refusal teaches); a dated id from the *source* is
//!   DE-DATED (refusing would throw away `claude-haiku-4-5`, which is the form
//!   transcripts actually write).
//! - **Absent is silent; present-but-rejected is said once.** A missing file is
//!   the normal state of every instance. A rejected row goes inert, its key
//!   falls through to the next tier, and [`PriceTable::diagnostic`] names it —
//!   once per distinct fingerprint in the log, and always in `GET /settings`.
//!
//! The single egress ([`fetch_source`]) lives here too, but strictly OUTSIDE the
//! read path: nothing in [`PriceTable::load`] touches the network.

use std::collections::BTreeMap;
use std::hash::Hasher;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tracing::warn;

/// The prices PDO ships — the former `run_cost::PRICES`, moved here verbatim.
///
/// Source: https://platform.claude.com/docs/en/about-claude/pricing (fetched
/// 2026-07-06). Per-MTok list prices `(family_key, input, output)`. Cache prices
/// are DERIVED (write_5m = 1.25×in, write_1h = 2×in, read = 0.1×in) — verified
/// universal across every current row. Match on the FULL family key: Opus
/// 4.5–4.8 are $5/$25 but Opus 4.1/4.0 are $15/$75 — never a
/// `starts_with("opus-4")` shortcut.
///
/// Since #427 this is the **floor**, not a seed: a sync never replaces it, and
/// three of these rows exist in no remote source at all.
const PRICES: &[(&str, f64, f64)] = &[
    ("claude-opus-4-8", 5.0, 25.0),
    ("claude-opus-4-7", 5.0, 25.0),
    ("claude-opus-4-6", 5.0, 25.0),
    ("claude-opus-4-5", 5.0, 25.0),
    ("claude-opus-4-1", 15.0, 75.0),
    ("claude-opus-4-0", 15.0, 75.0),
    ("claude-sonnet-4-6", 3.0, 15.0),
    ("claude-sonnet-4-5", 3.0, 15.0),
    ("claude-sonnet-4-0", 3.0, 15.0),
    ("claude-haiku-4-5", 1.0, 5.0),
    ("claude-3-5-haiku", 0.80, 4.0),
];

/// Claude Code's local/no-cost sentinel. Priced $0 **above every tier** so it can
/// never flip `partial` — ORDER IS LOAD-BEARING (see [`PriceTable::price_for`]).
const SYNTHETIC: &str = "<synthetic>";

/// The only schema marker `fetched.json` is read under. A document carrying
/// anything else is ENTIRELY inert — never a row read under a schema we do not
/// recognise (precedent: `hash_algo: "semantic-v1"`, `library_store.rs:259-269`).
pub(crate) const FETCHED_SCHEMA: &str = "prices-v1";

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

/// Drop a trailing 8-digit date segment so a dated id resolves to its family
/// key: `claude-sonnet-4-5-20250929` → `claude-sonnet-4-5`. A version-only id is
/// returned unchanged. Moved here from `run_cost.rs` — the fetch normaliser uses
/// it too.
pub(crate) fn strip_date_suffix(model: &str) -> &str {
    if let Some((head, tail)) = model.rsplit_once('-') {
        if tail.len() == 8 && tail.bytes().all(|b| b.is_ascii_digit()) {
            return head;
        }
    }
    model
}

/// Per-MTok `(input, output)` list price. `PartialEq` and not `Eq` — these are
/// `f64` (same reason as `CostStat`).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Price {
    pub input: f64,
    pub output: f64,
}

/// Which tier decided a family key. ADR-0015's vocabulary, transposed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PriceTier {
    Manual,
    Fetched,
    Embedded,
}

/// A refused row and why — the material of BOTH the `warn!` and the
/// `GET /settings` `reason`, so the two can never drift.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RejectedRow {
    pub key: String,
    pub why: String,
}

/// Where a fetched document came from and when. `None` for the manual tier: the
/// human's file carries no provenance, by design (PDO never writes it).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Provenance {
    pub source: String,
    pub fetched_at: String,
}

/// The result of parsing one on-disk tier. Pure: text in, rows + refusals out.
/// An unparseable document yields `unparseable: Some(err)` and NO rows — never an
/// `Err`, because a bad price file must not be able to fail a cost read.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct ParsedTier {
    pub rows: BTreeMap<String, Price>,
    pub rejected: Vec<RejectedRow>,
    pub unparseable: Option<String>,
    pub provenance: Option<Provenance>,
}

impl ParsedTier {
    /// A tier built straight from rows — the shape a test or the sync writer
    /// wants when there is nothing to refuse.
    #[cfg(test)]
    fn of(rows: &[(&str, f64, f64)]) -> Self {
        Self {
            rows: rows
                .iter()
                .map(|(k, i, o)| {
                    (
                        (*k).to_string(),
                        Price {
                            input: *i,
                            output: *o,
                        },
                    )
                })
                .collect(),
            ..Default::default()
        }
    }
}

/// The table resolved for one read, plus the fingerprint saying WHICH table it
/// is. That fingerprint is the whole reason this is not a bare map: it is the
/// third component of `run_cost`'s memo key, without which a sync would stay
/// invisible on `/stats/cost` until the daemon restarted.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PriceTable {
    resolved: BTreeMap<String, (Price, PriceTier)>,
    manual: ParsedTier,
    fetched: ParsedTier,
    manual_keys: Vec<String>,
    fingerprint: u64,
    /// Set by [`Self::load`] only, so `diagnostic()` can name the real file.
    /// `None` for a table built by the pure [`Self::resolve`].
    manual_path: Option<PathBuf>,
    fetched_path: Option<PathBuf>,
}

impl Default for PriceTable {
    fn default() -> Self {
        Self::builtin()
    }
}

impl PriceTable {
    /// The floor alone. Pure, no IO, no `$HOME`. `fingerprint() == 0`.
    pub(crate) fn builtin() -> Self {
        Self::resolve(ParsedTier::default(), ParsedTier::default(), 0)
    }

    /// PURE, TOTAL resolver — tiers injected, zero IO. Mirrors
    /// `sandbox_profile::resolve_entry_list`: ONE resolver shared by the cost
    /// computation and by the `GET /settings` view, so what the UI shows can
    /// never drift from what actually prices (the lesson of #373).
    pub(crate) fn resolve(manual: ParsedTier, fetched: ParsedTier, fingerprint: u64) -> Self {
        let mut resolved: BTreeMap<String, (Price, PriceTier)> = BTreeMap::new();
        // Lowest precedence first, so a higher tier simply overwrites.
        for (k, i, o) in PRICES {
            resolved.insert(
                (*k).to_string(),
                (
                    Price {
                        input: *i,
                        output: *o,
                    },
                    PriceTier::Embedded,
                ),
            );
        }
        for (k, p) in &fetched.rows {
            resolved.insert(k.clone(), (*p, PriceTier::Fetched));
        }
        for (k, p) in &manual.rows {
            resolved.insert(k.clone(), (*p, PriceTier::Manual));
        }
        let manual_keys = manual.rows.keys().cloned().collect();
        Self {
            resolved,
            manual,
            fetched,
            manual_keys,
            fingerprint,
            manual_path: None,
            fetched_path: None,
        }
    }

    /// `<home_root>/.pdo/prices/{models.yaml, fetched.json}` — path arithmetic
    /// only, like `sandbox_image::default_dockerfile_path(sandbox_root)`.
    pub(crate) fn paths(home_root: &Path) -> (PathBuf, PathBuf) {
        let dir = home_root.join(".pdo").join("prices");
        (dir.join("models.yaml"), dir.join("fetched.json"))
    }

    /// Load the effective table from an injected root. Absent or unreadable →
    /// that tier is empty and SILENT (the normal state of every instance).
    /// Unparseable, or an unknown `schema` → the tier is ENTIRELY inert plus a
    /// diagnostic. Emits AT MOST ONE `warn!` per distinct fingerprint, so a
    /// polled `/stats/cost` cannot produce one log line per request.
    pub(crate) fn load(home_root: &Path) -> Self {
        let (manual_path, fetched_path) = Self::paths(home_root);
        let manual_bytes = std::fs::read(&manual_path).ok();
        let fetched_bytes = std::fs::read(&fetched_path).ok();
        let fingerprint = fingerprint_of(manual_bytes.as_deref(), fetched_bytes.as_deref());

        let manual = manual_bytes
            .as_deref()
            .map(|b| parse_manual(&String::from_utf8_lossy(b)))
            .unwrap_or_default();
        let fetched = fetched_bytes
            .as_deref()
            .map(|b| parse_fetched(&String::from_utf8_lossy(b)))
            .unwrap_or_default();

        let mut table = Self::resolve(manual, fetched, fingerprint);
        table.manual_path = Some(manual_path);
        table.fetched_path = Some(fetched_path);
        table.warn_once();
        table
    }

    /// Per-MTok price, or `None` for a model unknown to all THREE tiers (the
    /// caller then flips `partial` and the line contributes $0).
    ///
    /// `<synthetic>` is priced $0 **above every table** — ORDER IS LOAD-BEARING.
    /// Moving the lookup first would let a price file turn the sentinel into a
    /// real cost, and would break the `Some((0,0))` / `None` distinction that
    /// keeps `partial` honest.
    pub(crate) fn price_for(&self, model: &str) -> Option<Price> {
        if model == SYNTHETIC {
            return Some(Price {
                input: 0.0,
                output: 0.0,
            });
        }
        self.resolved.get(strip_date_suffix(model)).map(|(p, _)| *p)
    }

    /// Which tier decides a family key, or `None` if no tier knows it. For the
    /// view and for the sync report.
    pub(crate) fn tier_of(&self, key: &str) -> Option<PriceTier> {
        self.resolved.get(key).map(|(_, t)| *t)
    }

    /// The resolved table, one `(family key, winning price, deciding tier)` per
    /// entry, in `BTreeMap` order. The SAME map `price_for` reads, so the
    /// `GET /settings` view can never enumerate a set the pricer would price
    /// otherwise (#373; cf. the doc-comment on `resolve`). Yields by value:
    /// `Price` and `PriceTier` are `Copy`. Exposes neither the container nor the
    /// internal `(Price, PriceTier)` tuple as a `pub(crate)` contract, so the
    /// wire JSON stays assembled in the axum layer.
    pub(crate) fn resolved_entries(&self) -> impl Iterator<Item = (&str, Price, PriceTier)> + '_ {
        self.resolved.iter().map(|(k, (p, t))| (k.as_str(), *p, *t))
    }

    pub(crate) fn fingerprint(&self) -> u64 {
        self.fingerprint
    }

    /// The keys the manual tier actually decided — the `GET /settings` signal
    /// that a hand edit is shadowing a sync.
    pub(crate) fn manual_keys(&self) -> &[String] {
        &self.manual_keys
    }

    /// Rows accepted from `fetched.json` (0 when the file is absent or inert).
    pub(crate) fn fetched_rows(&self) -> usize {
        self.fetched.rows.len()
    }

    /// The fetched tier's rows, for the sync's added/updated/unchanged diff.
    pub(crate) fn fetched_prices(&self) -> &BTreeMap<String, Price> {
        &self.fetched.rows
    }

    /// Vintage of the fetched tier — D14 pt 2: the table's date is readable, not
    /// guessed.
    pub(crate) fn fetched_at(&self) -> Option<&str> {
        self.fetched
            .provenance
            .as_ref()
            .map(|p| p.fetched_at.as_str())
    }

    /// URL of the last successful fetch.
    pub(crate) fn source(&self) -> Option<&str> {
        self.fetched.provenance.as_ref().map(|p| p.source.as_str())
    }

    /// ONE message naming every inert file and every refused row, or `None` when
    /// nothing is wrong. PURE, so a unit test can pin it instead of a terminal
    /// (modelled on `retired_sandbox_settings_warning`). ADR-0015:44: two lines
    /// for one problem read as two problems.
    pub(crate) fn diagnostic(&self) -> Option<String> {
        let mut parts: Vec<String> = Vec::new();
        for (tier, parsed, path) in [
            ("manual", &self.manual, self.manual_path.as_ref()),
            ("fetched", &self.fetched, self.fetched_path.as_ref()),
        ] {
            let where_ = match path {
                Some(p) => format!("{} price tier ({})", tier, p.display()),
                None => format!("{tier} price tier"),
            };
            if let Some(err) = &parsed.unparseable {
                parts.push(format!("{where_} is entirely inert: {err}"));
            }
            if !parsed.rejected.is_empty() {
                let rows = parsed
                    .rejected
                    .iter()
                    .map(|r| format!("`{}` ({})", r.key, r.why))
                    .collect::<Vec<_>>()
                    .join(", ");
                parts.push(format!(
                    "{where_} refused {} row(s), each falling through to the next tier: {rows}",
                    parsed.rejected.len()
                ));
            }
        }
        if parts.is_empty() {
            None
        } else {
            Some(format!("price table (#427) — {}", parts.join(" ; ")))
        }
    }

    /// Say a diagnostic at most once per distinct table. A *differently* bad file
    /// has a different fingerprint and so is said again.
    ///
    /// A `static` de-dup is a novation here — the crate's two conventions are
    /// unique *placement* (`warn_about_retired_sandbox_settings`, called once at
    /// boot) and de-dup through the *event log* (`stale_detector.rs:653`).
    /// Neither applies to a loader called once per request.
    fn warn_once(&self) {
        static LAST_WARNED: Mutex<Option<u64>> = Mutex::new(None);
        let Some(msg) = self.diagnostic() else {
            return;
        };
        let mut guard = LAST_WARNED.lock().unwrap_or_else(|e| e.into_inner());
        if *guard == Some(self.fingerprint) {
            return;
        }
        *guard = Some(self.fingerprint);
        warn!("{msg}");
    }
}

/// Content hash of the bytes actually read. `0` ⇔ both files absent.
///
/// Preferred over mtime: no millisecond granularity to reason about and no
/// copy-preserves-mtime trap. The hasher need NOT be stable across Rust versions
/// — the only consumer is `run_cost`'s process-local RAM memo.
fn fingerprint_of(manual: Option<&[u8]>, fetched: Option<&[u8]>) -> u64 {
    if manual.is_none() && fetched.is_none() {
        return 0;
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for part in [manual, fetched] {
        match part {
            // Length-prefixed so `("ab", "c")` and `("a", "bc")` cannot collide.
            Some(b) => {
                h.write_u8(1);
                h.write_usize(b.len());
                h.write(b);
            }
            None => h.write_u8(0),
        }
    }
    // `0` is reserved for "no file at all", so a real table never claims it.
    match h.finish() {
        0 => 1,
        v => v,
    }
}

/// Validate one row's `(input, output)` numbers. Applied to BOTH disk tiers: a
/// `NaN` would poison `usd` *and* serialise to JSON `null` for a frontend that
/// types it `number`, and a negative price would silently deflate a total.
/// **Refuse, never clamp** — `CONTEXT.md:813` forbids silent self-repair, and
/// `PUT /settings` 400s a cap `< 1` rather than clamping it.
fn validate_price(input: f64, output: f64) -> Result<Price, String> {
    for (label, v) in [("input", input), ("output", output)] {
        if !v.is_finite() {
            return Err(format!("{label} price is not a finite number"));
        }
        if v < 0.0 {
            return Err(format!("{label} price is negative"));
        }
    }
    Ok(Price { input, output })
}

/// Reject a family key that could never price anything. Shared by both tiers'
/// key checks that are *not* about de-dating.
fn reject_sentinel_key(key: &str) -> Option<String> {
    key.starts_with('<').then(|| {
        format!(
            "`{key}` looks like a Claude Code sentinel (`{SYNTHETIC}` is priced $0 above every \
             tier, so such a row could only ever be inert)"
        )
    })
}

/// Parse the manual tier (`models.yaml`). Applies D4's four refusal rules row by
/// row. PURE.
///
/// Shape — a MAP, not a list of records, so key uniqueness is structural and the
/// file reads as what it is, a sparse patch:
///
/// ```yaml
/// models:
///   claude-opus-4-8: { input: 4.5, output: 22.5 }
/// ```
///
/// An unknown *field* stays ignored (no `deny_unknown_fields` anywhere in this
/// crate; ADR-0015 #471 says an unknown field is simply ignored by serde).
pub(crate) fn parse_manual(text: &str) -> ParsedTier {
    #[derive(serde::Deserialize)]
    struct Doc {
        #[serde(default)]
        models: BTreeMap<String, serde_yaml::Value>,
    }

    // A blank/comment-only file deserialises to YAML null, which is not a `Doc`.
    // That is an empty patch, not a broken one.
    if text.trim().is_empty() {
        return ParsedTier::default();
    }
    let doc: Doc = match serde_yaml::from_str(text) {
        Ok(d) => d,
        Err(e) => {
            return ParsedTier {
                unparseable: Some(e.to_string()),
                ..Default::default()
            }
        }
    };

    let mut tier = ParsedTier::default();
    for (key, value) in doc.models {
        // Rule 1 — a dated key would NEVER price anything, and the symptom is
        // indistinguishable from absence. Refuse, do not normalise: stripping
        // would silently collapse two rows the author wanted distinct. The
        // message prints the correct form.
        let family = strip_date_suffix(&key);
        if family != key {
            tier.rejected.push(RejectedRow {
                key: key.clone(),
                why: format!(
                    "keys are FAMILY keys and carry no date suffix — write `{family}` instead"
                ),
            });
            continue;
        }
        // Rule 2 — a sentinel-shaped key.
        if let Some(why) = reject_sentinel_key(&key) {
            tier.rejected.push(RejectedRow { key, why });
            continue;
        }
        // Rule 3 — the numbers. A missing or wrongly-typed field lands here too.
        let num = |field: &str| -> Result<f64, String> {
            value
                .get(field)
                .ok_or_else(|| format!("missing `{field}`"))?
                .as_f64()
                .ok_or_else(|| format!("`{field}` is not a number"))
        };
        match num("input").and_then(|i| num("output").and_then(|o| validate_price(i, o))) {
            Ok(price) => {
                tier.rows.insert(key, price);
            }
            Err(why) => tier.rejected.push(RejectedRow { key, why }),
        }
        // Rule 4 — a duplicate key is structurally impossible: the schema is a map.
    }
    tier
}

/// Parse the fetched tier (`fetched.json`). Requires `schema == "prices-v1"`;
/// anything else leaves the tier ENTIRELY inert. PURE.
pub(crate) fn parse_fetched(text: &str) -> ParsedTier {
    #[derive(serde::Deserialize)]
    struct Doc {
        schema: Option<String>,
        source: Option<String>,
        fetched_at: Option<String>,
        #[serde(default)]
        models: BTreeMap<String, Price>,
    }

    if text.trim().is_empty() {
        return ParsedTier {
            unparseable: Some("file is empty".to_string()),
            ..Default::default()
        };
    }
    let doc: Doc = match serde_json::from_str(text) {
        Ok(d) => d,
        Err(e) => {
            return ParsedTier {
                unparseable: Some(e.to_string()),
                ..Default::default()
            }
        }
    };
    match doc.schema.as_deref() {
        Some(FETCHED_SCHEMA) => {}
        other => {
            return ParsedTier {
                unparseable: Some(format!(
                    "unrecognised schema {} — expected `{FETCHED_SCHEMA}`; no row is read under a \
                     schema this build does not know",
                    other.map_or("(absent)".to_string(), |s| format!("`{s}`"))
                )),
                ..Default::default()
            }
        }
    }

    let mut tier = ParsedTier {
        provenance: match (doc.source, doc.fetched_at) {
            (Some(source), Some(fetched_at)) => Some(Provenance { source, fetched_at }),
            _ => None,
        },
        ..Default::default()
    };
    // The daemon validated these at write time, so rows are taken as written —
    // except for the numeric guard, which is free and defends a hand-edited file
    // (the name says the daemon owns it, but nothing enforces that).
    for (key, price) in doc.models {
        if let Some(why) = reject_sentinel_key(&key) {
            tier.rejected.push(RejectedRow { key, why });
            continue;
        }
        match validate_price(price.input, price.output) {
            Ok(p) => {
                tier.rows.insert(key, p);
            }
            Err(why) => tier.rejected.push(RejectedRow { key, why }),
        }
    }
    tier
}

// --- The single egress, strictly outside the read path (ADR-0034) ------------

/// GET the price source. The daemon's ONLY outbound call outside `docker pull`
/// and the shelled Trigger guards.
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

/// Monotonic counter for unique temp names, like `sandbox_staging`'s `TMP_SEQ`.
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Serialise and write `fetched.json` by tmp + `rename` **in the same
/// directory** (idiom of `sandbox_staging.rs:790`), so a concurrent
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

    fn p(input: f64, output: f64) -> Price {
        Price { input, output }
    }

    // --- strip_date_suffix (moved from run_cost.rs with its tests) ---

    #[test]
    fn strips_trailing_8_digit_date() {
        assert_eq!(
            strip_date_suffix("claude-sonnet-4-5-20250929"),
            "claude-sonnet-4-5"
        );
        assert_eq!(
            strip_date_suffix("claude-3-5-haiku-20241022"),
            "claude-3-5-haiku"
        );
    }

    #[test]
    fn leaves_version_only_id_untouched() {
        assert_eq!(strip_date_suffix("claude-opus-4-8"), "claude-opus-4-8");
        assert_eq!(strip_date_suffix("claude-opus-5"), "claude-opus-5");
    }

    // --- price_for on the builtin floor (the moved run_cost tests) ---

    #[test]
    fn prices_known_models() {
        let t = PriceTable::builtin();
        assert_eq!(t.price_for("claude-opus-4-8"), Some(p(5.0, 25.0)));
        assert_eq!(t.price_for("claude-sonnet-4-5"), Some(p(3.0, 15.0)));
        assert_eq!(t.price_for("claude-haiku-4-5"), Some(p(1.0, 5.0)));
    }

    #[test]
    fn opus_4_1_and_4_0_are_not_collapsed_with_4_5_plus() {
        // The single most error-prone row: same "opus-4" prefix, different price.
        let t = PriceTable::builtin();
        assert_eq!(t.price_for("claude-opus-4-1"), Some(p(15.0, 75.0)));
        assert_eq!(t.price_for("claude-opus-4-0"), Some(p(15.0, 75.0)));
        assert_ne!(
            t.price_for("claude-opus-4-1"),
            t.price_for("claude-opus-4-8")
        );
    }

    #[test]
    fn dated_id_resolves_to_family_price() {
        assert_eq!(
            PriceTable::builtin().price_for("claude-sonnet-4-5-20250929"),
            Some(p(3.0, 15.0))
        );
    }

    #[test]
    fn synthetic_is_zero_not_unknown() {
        assert_eq!(
            PriceTable::builtin().price_for(SYNTHETIC),
            Some(p(0.0, 0.0))
        );
    }

    #[test]
    fn unknown_model_is_none() {
        let t = PriceTable::builtin();
        assert_eq!(t.price_for("gpt-9"), None);
        assert_eq!(t.price_for("claude-opus-9-9"), None);
    }

    #[test]
    fn builtin_has_a_zero_fingerprint_and_no_diagnostic() {
        let t = PriceTable::builtin();
        assert_eq!(t.fingerprint(), 0);
        assert_eq!(t.diagnostic(), None);
        assert!(t.manual_keys().is_empty());
        assert_eq!(t.fetched_rows(), 0);
        assert_eq!(t.fetched_at(), None);
        assert_eq!(t.source(), None);
    }

    // --- parse_manual ---

    #[test]
    fn parse_manual_accepts_a_valid_map() {
        let tier = parse_manual(
            "models:\n  claude-opus-4-8: { input: 4.5, output: 22.5 }\n  claude-mythos-5: { input: 10.0, output: 50.0 }\n",
        );
        assert!(tier.unparseable.is_none());
        assert!(tier.rejected.is_empty());
        assert_eq!(tier.rows.get("claude-opus-4-8"), Some(&p(4.5, 22.5)));
        assert_eq!(tier.rows.get("claude-mythos-5"), Some(&p(10.0, 50.0)));
    }

    #[test]
    fn parse_manual_refuses_a_dated_key_and_prints_the_correct_form() {
        let tier =
            parse_manual("models:\n  claude-opus-5-20260501: { input: 5.0, output: 25.0 }\n");
        assert!(tier.rows.is_empty(), "a dated key must not price anything");
        assert_eq!(tier.rejected.len(), 1);
        assert_eq!(tier.rejected[0].key, "claude-opus-5-20260501");
        assert!(
            tier.rejected[0].why.contains("claude-opus-5"),
            "the refusal must print the correct undated form, got: {}",
            tier.rejected[0].why
        );
        // And it must print the FAMILY form, not merely echo the dated key.
        assert!(!tier.rejected[0].why.contains("`claude-opus-5-20260501`"));
    }

    #[test]
    fn parse_manual_refuses_a_sentinel_key() {
        let tier = parse_manual("models:\n  \"<synthetic>\": { input: 99.0, output: 99.0 }\n");
        assert!(tier.rows.is_empty());
        assert_eq!(tier.rejected.len(), 1);
        assert_eq!(tier.rejected[0].key, SYNTHETIC);
    }

    #[test]
    fn parse_manual_refuses_negative_and_non_finite_prices() {
        let neg = parse_manual("models:\n  claude-opus-5: { input: -1.0, output: 25.0 }\n");
        assert!(neg.rows.is_empty());
        assert!(neg.rejected[0].why.contains("negative"));

        let nan = parse_manual("models:\n  claude-opus-5: { input: 5.0, output: .nan }\n");
        assert!(
            nan.rows.is_empty(),
            "a NaN would poison `usd` and serialise as JSON null"
        );
        assert!(nan.rejected[0].why.contains("finite"));

        let inf = parse_manual("models:\n  claude-opus-5: { input: .inf, output: 25.0 }\n");
        assert!(inf.rows.is_empty());
    }

    #[test]
    fn parse_manual_refuses_a_row_with_a_missing_field() {
        let tier = parse_manual("models:\n  claude-opus-5: { input: 5.0 }\n");
        assert!(tier.rows.is_empty());
        assert!(tier.rejected[0].why.contains("output"));
    }

    #[test]
    fn parse_manual_ignores_an_unknown_field_rather_than_rejecting() {
        // No `deny_unknown_fields` anywhere in this crate (ADR-0015 #471).
        let tier = parse_manual(
            "models:\n  claude-opus-5: { input: 5.0, output: 25.0, note: \"discount\" }\n",
        );
        assert!(tier.rejected.is_empty());
        assert_eq!(tier.rows.get("claude-opus-5"), Some(&p(5.0, 25.0)));
    }

    #[test]
    fn parse_manual_reports_broken_yaml_without_rows() {
        let tier = parse_manual("models:\n  - this is: [not, a, map\n");
        assert!(tier.unparseable.is_some());
        assert!(tier.rows.is_empty());
    }

    #[test]
    fn parse_manual_treats_an_empty_or_comment_only_file_as_an_empty_patch() {
        // A user who creates the file from the path shown in Settings and has not
        // written a row yet must NOT see a diagnostic: that would read as a defect.
        for text in ["", "   \n", "# nothing yet\n", "models:\n", "models: {}\n"] {
            let tier = parse_manual(text);
            assert!(
                tier.unparseable.is_none(),
                "text = {text:?} → {:?}",
                tier.unparseable
            );
            assert!(tier.rows.is_empty());
            assert!(tier.rejected.is_empty());
        }
    }

    #[test]
    fn parse_manual_keeps_the_good_rows_when_one_is_refused() {
        let tier = parse_manual(
            "models:\n  claude-opus-4-8: { input: 4.5, output: 22.5 }\n  claude-opus-5-20260501: { input: 5.0, output: 25.0 }\n",
        );
        assert_eq!(tier.rows.len(), 1, "one bad row must not void the file");
        assert_eq!(tier.rejected.len(), 1);
    }

    // --- parse_fetched ---

    fn fetched_doc(schema: &str) -> String {
        format!(
            r#"{{"schema":"{schema}","source":"https://models.dev/api.json","fetched_at":"2026-07-30T14:12:03Z","models":{{"claude-opus-5":{{"input":5.0,"output":25.0}}}}}}"#
        )
    }

    #[test]
    fn parse_fetched_accepts_the_known_schema_with_its_provenance() {
        let tier = parse_fetched(&fetched_doc(FETCHED_SCHEMA));
        assert!(tier.unparseable.is_none());
        assert_eq!(tier.rows.get("claude-opus-5"), Some(&p(5.0, 25.0)));
        assert_eq!(
            tier.provenance.as_ref().map(|x| x.fetched_at.as_str()),
            Some("2026-07-30T14:12:03Z")
        );
    }

    #[test]
    fn parse_fetched_is_entirely_inert_under_an_unknown_or_absent_schema() {
        for text in [
            fetched_doc("prices-v99"),
            r#"{"models":{"claude-opus-5":{"input":5.0,"output":25.0}}}"#.to_string(),
        ] {
            let tier = parse_fetched(&text);
            assert!(
                tier.rows.is_empty(),
                "no row may be read under a schema this build does not know"
            );
            assert!(tier.unparseable.is_some());
        }
    }

    #[test]
    fn parse_fetched_is_inert_on_broken_or_empty_json() {
        for text in ["{not json", ""] {
            let tier = parse_fetched(text);
            assert!(tier.rows.is_empty());
            assert!(tier.unparseable.is_some());
        }
    }

    #[test]
    fn parse_fetched_still_guards_the_numbers_of_a_hand_edited_file() {
        let text = r#"{"schema":"prices-v1","source":"s","fetched_at":"t","models":{"claude-opus-5":{"input":-5.0,"output":25.0}}}"#;
        let tier = parse_fetched(text);
        assert!(tier.rows.is_empty());
        assert_eq!(tier.rejected.len(), 1);
    }

    // --- resolve: precedence, by key ---

    #[test]
    fn manual_wins_over_fetched_which_wins_over_embedded() {
        let t = PriceTable::resolve(
            ParsedTier::of(&[("claude-opus-4-8", 1.0, 1.0)]),
            ParsedTier::of(&[("claude-opus-4-8", 2.0, 2.0), ("claude-opus-4-7", 3.0, 3.0)]),
            42,
        );
        assert_eq!(t.price_for("claude-opus-4-8"), Some(p(1.0, 1.0)));
        assert_eq!(t.tier_of("claude-opus-4-8"), Some(PriceTier::Manual));
        assert_eq!(t.price_for("claude-opus-4-7"), Some(p(3.0, 3.0)));
        assert_eq!(t.tier_of("claude-opus-4-7"), Some(PriceTier::Fetched));
        // Untouched by either disk tier → still the embedded floor.
        assert_eq!(t.price_for("claude-opus-4-6"), Some(p(5.0, 25.0)));
        assert_eq!(t.tier_of("claude-opus-4-6"), Some(PriceTier::Embedded));
        assert_eq!(t.tier_of("claude-nope"), None);
        assert_eq!(t.fingerprint(), 42);
        assert_eq!(t.manual_keys(), ["claude-opus-4-8"]);
    }

    #[test]
    fn a_key_only_the_disk_knows_is_priced_and_no_longer_partial() {
        let t = PriceTable::resolve(
            ParsedTier::default(),
            ParsedTier::of(&[("claude-fable-5", 10.0, 50.0)]),
            7,
        );
        assert_eq!(t.price_for("claude-fable-5"), Some(p(10.0, 50.0)));
        // Merge BY KEY: the rest of the floor survives a partial disk tier.
        assert_eq!(t.price_for("claude-opus-4-8"), Some(p(5.0, 25.0)));
    }

    #[test]
    fn the_embedded_tier_is_a_floor_not_a_seed() {
        // #427 D2: these three families are in NO remote source, so a full
        // fetched tier must not be able to un-price them.
        let full_fetch = ParsedTier::of(&[
            ("claude-opus-4-8", 5.0, 25.0),
            ("claude-opus-5", 5.0, 25.0),
            ("claude-sonnet-5", 2.0, 10.0),
            ("claude-fable-5", 10.0, 50.0),
        ]);
        let t = PriceTable::resolve(ParsedTier::default(), full_fetch, 9);
        assert_eq!(t.price_for("claude-opus-4-0"), Some(p(15.0, 75.0)));
        assert_eq!(t.price_for("claude-sonnet-4-0"), Some(p(3.0, 15.0)));
        assert_eq!(t.price_for("claude-3-5-haiku"), Some(p(0.80, 4.0)));
        for k in ["claude-opus-4-0", "claude-sonnet-4-0", "claude-3-5-haiku"] {
            assert_eq!(t.tier_of(k), Some(PriceTier::Embedded), "key = {k}");
        }
    }

    #[test]
    fn a_refused_row_falls_through_to_the_next_tier_instead_of_destroying_it() {
        // D5: a typo in the manual file must not collapse 79 941 lines.
        let manual =
            parse_manual("models:\n  claude-opus-4-8-20260101: { input: 4.5, output: 22.5 }\n");
        assert_eq!(manual.rejected.len(), 1);
        let t = PriceTable::resolve(manual, ParsedTier::of(&[("claude-opus-4-8", 2.0, 2.0)]), 3);
        assert_eq!(t.price_for("claude-opus-4-8"), Some(p(2.0, 2.0)));
        assert_eq!(t.tier_of("claude-opus-4-8"), Some(PriceTier::Fetched));

        // And with no fetched tier at all, it falls all the way to the floor.
        let manual =
            parse_manual("models:\n  claude-opus-4-8-20260101: { input: 4.5, output: 22.5 }\n");
        let t = PriceTable::resolve(manual, ParsedTier::default(), 3);
        assert_eq!(t.price_for("claude-opus-4-8"), Some(p(5.0, 25.0)));
    }

    #[test]
    fn synthetic_stays_zero_even_when_a_file_prices_it_at_99() {
        // The test that stops a future refactor from moving the lookup above the
        // sentinel guard. `parse_manual` refuses such a row, so build the tier
        // directly — this pins `price_for`'s ORDER, not the parser.
        let t = PriceTable::resolve(
            ParsedTier::of(&[(SYNTHETIC, 99.0, 99.0)]),
            ParsedTier::default(),
            1,
        );
        assert_eq!(t.price_for(SYNTHETIC), Some(p(0.0, 0.0)));
    }

    // --- resolved_entries: the read view (#528) ---

    #[test]
    fn resolved_entries_on_builtin_are_the_eleven_embedded_families() {
        let rows: Vec<(String, Price, PriceTier)> = PriceTable::builtin()
            .resolved_entries()
            .map(|(k, p, t)| (k.to_string(), p, t))
            .collect();
        // Exactly the floor, no more, no less.
        assert_eq!(rows.len(), PRICES.len());
        assert_eq!(rows.len(), 11);
        // Every floor line is the embedded tier.
        assert!(rows.iter().all(|(_, _, t)| *t == PriceTier::Embedded));
        // The most error-prone distinction survives round-tripping through the
        // accessor: opus-4-8 at (5,25) is not opus-4-1 at (15,75).
        let by = |key: &str| rows.iter().find(|(k, ..)| k == key).map(|(_, p, _)| *p);
        assert_eq!(by("claude-opus-4-8"), Some(p(5.0, 25.0)));
        assert_eq!(by("claude-opus-4-1"), Some(p(15.0, 75.0)));
        assert_eq!(by("claude-haiku-4-5"), Some(p(1.0, 5.0)));
    }

    #[test]
    fn resolved_entries_report_the_winning_tier_per_family() {
        let t = PriceTable::resolve(
            ParsedTier::of(&[("claude-opus-4-8", 4.5, 22.5)]),
            ParsedTier::of(&[("claude-opus-5", 5.0, 25.0)]),
            7,
        );
        let rows: Vec<(String, Price, PriceTier)> = t
            .resolved_entries()
            .map(|(k, p, t)| (k.to_string(), p, t))
            .collect();
        let row = |key: &str| rows.iter().find(|(k, ..)| k == key).cloned();
        // Manually overridden family: winning price + Manual tier.
        assert_eq!(
            row("claude-opus-4-8"),
            Some((
                "claude-opus-4-8".to_string(),
                p(4.5, 22.5),
                PriceTier::Manual
            ))
        );
        // Fetch-only family: Fetched tier.
        assert_eq!(
            row("claude-opus-5"),
            Some((
                "claude-opus-5".to_string(),
                p(5.0, 25.0),
                PriceTier::Fetched
            ))
        );
        // Untouched family: still the embedded floor.
        assert_eq!(
            row("claude-opus-4-7"),
            Some((
                "claude-opus-4-7".to_string(),
                p(5.0, 25.0),
                PriceTier::Embedded
            ))
        );
    }

    #[test]
    fn resolved_entries_never_emit_the_synthetic_sentinel() {
        // The sentinel is kept out of the resolved table by the PARSERS refusing it
        // (`resolve` inserts every tier row blindly), so a file that tries to price
        // `<synthetic>` yields no such row — and hence no such entry in the view.
        let manual = parse_manual("models:\n  \"<synthetic>\": { input: 99.0, output: 99.0 }\n");
        assert!(
            manual.rows.is_empty(),
            "the parser must refuse the sentinel"
        );
        let t = PriceTable::resolve(manual, ParsedTier::default(), 1);
        assert!(t.resolved_entries().all(|(k, ..)| k != SYNTHETIC));
    }

    #[test]
    fn resolved_entries_come_out_in_btreemap_key_order() {
        let table = PriceTable::builtin();
        let keys: Vec<&str> = table.resolved_entries().map(|(k, ..)| k).collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(keys, sorted, "the view must inherit the BTreeMap ordering");
    }

    // --- diagnostic ---

    #[test]
    fn diagnostic_is_none_on_a_healthy_table() {
        let t = PriceTable::resolve(
            ParsedTier::of(&[("claude-opus-4-8", 1.0, 1.0)]),
            ParsedTier::of(&[("claude-opus-5", 5.0, 25.0)]),
            5,
        );
        assert_eq!(t.diagnostic(), None);
    }

    #[test]
    fn diagnostic_is_one_string_naming_every_inert_file_and_row() {
        // ADR-0015:44 — two lines for one problem read as two problems.
        let t = PriceTable::resolve(
            parse_manual("models:\n  claude-opus-5-20260501: { input: 5.0, output: 25.0 }\n  claude-x: { input: -1.0, output: 2.0 }\n"),
            parse_fetched(&fetched_doc("prices-v99")),
            5,
        );
        let d = t.diagnostic().expect("a diagnostic was expected");
        assert_eq!(d.lines().count(), 1, "exactly one message: {d}");
        assert!(d.contains("claude-opus-5-20260501"));
        assert!(d.contains("claude-x"));
        assert!(d.contains("prices-v99"));
        assert!(d.contains("manual") && d.contains("fetched"));
    }

    // --- fingerprint ---

    #[test]
    fn fingerprint_is_zero_only_when_both_files_are_absent() {
        assert_eq!(fingerprint_of(None, None), 0);
        assert_ne!(fingerprint_of(Some(b""), None), 0);
        assert_ne!(fingerprint_of(None, Some(b"")), 0);
    }

    #[test]
    fn fingerprint_changes_with_content_and_cannot_be_confused_by_a_shifted_split() {
        assert_ne!(
            fingerprint_of(Some(b"a"), None),
            fingerprint_of(Some(b"b"), None)
        );
        // Length-prefixing: ("ab", "c") and ("a", "bc") must not collide.
        assert_ne!(
            fingerprint_of(Some(b"ab"), Some(b"c")),
            fingerprint_of(Some(b"a"), Some(b"bc"))
        );
        // Which file the bytes came from matters too.
        assert_ne!(
            fingerprint_of(Some(b"x"), None),
            fingerprint_of(None, Some(b"x"))
        );
    }

    // --- load (filesystem, injected root — no $HOME swap) ---

    fn write_manual(home: &Path, body: &str) {
        let (path, _) = PriceTable::paths(home);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    fn write_fetched_raw(home: &Path, body: &str) {
        let (_, path) = PriceTable::paths(home);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    #[test]
    fn load_on_a_root_without_a_prices_dir_is_the_builtin_floor_and_silent() {
        let home = tempfile::tempdir().unwrap();
        let t = PriceTable::load(home.path());
        assert_eq!(t.fingerprint(), 0);
        assert_eq!(t.diagnostic(), None);
        assert_eq!(t.price_for("claude-opus-4-8"), Some(p(5.0, 25.0)));
        assert_eq!(t.price_for("claude-opus-5"), None);
        // Byte-identical to the pre-#427 behaviour.
        assert_eq!(t.resolved, PriceTable::builtin().resolved);
    }

    #[test]
    fn load_applies_the_manual_tier_and_keeps_the_rest_of_the_floor() {
        let home = tempfile::tempdir().unwrap();
        write_manual(
            home.path(),
            "models:\n  claude-opus-5: { input: 5.0, output: 25.0 }\n",
        );
        let t = PriceTable::load(home.path());
        assert_eq!(t.price_for("claude-opus-5"), Some(p(5.0, 25.0)));
        assert_eq!(t.price_for("claude-opus-4-8"), Some(p(5.0, 25.0)));
        assert_ne!(t.fingerprint(), 0);
        assert_eq!(t.manual_keys(), ["claude-opus-5"]);
    }

    #[test]
    fn load_of_a_corrupt_fetched_file_prices_like_absence_but_says_so() {
        let home = tempfile::tempdir().unwrap();
        write_fetched_raw(home.path(), "{not json");
        let t = PriceTable::load(home.path());
        assert_eq!(t.price_for("claude-opus-4-8"), Some(p(5.0, 25.0)));
        assert_eq!(t.price_for("claude-opus-5"), None);
        let d = t.diagnostic().expect("a corrupt file must be said");
        assert!(d.contains("fetched.json"), "the path must be named: {d}");
    }

    #[test]
    fn load_reads_both_tiers_with_manual_winning() {
        let home = tempfile::tempdir().unwrap();
        write_manual(
            home.path(),
            "models:\n  claude-opus-5: { input: 4.0, output: 20.0 }\n",
        );
        write_fetched_raw(home.path(), &fetched_doc(FETCHED_SCHEMA));
        let t = PriceTable::load(home.path());
        assert_eq!(t.price_for("claude-opus-5"), Some(p(4.0, 20.0)));
        assert_eq!(t.tier_of("claude-opus-5"), Some(PriceTier::Manual));
        assert_eq!(t.fetched_rows(), 1);
        assert_eq!(t.source(), Some("https://models.dev/api.json"));
        assert_eq!(t.fetched_at(), Some("2026-07-30T14:12:03Z"));
    }

    #[test]
    fn load_names_both_paths_even_when_no_file_exists() {
        // The whole discoverability story: nothing is seeded, so `GET /settings`
        // naming the paths is the only way a user learns where to write.
        let home = tempfile::tempdir().unwrap();
        let t = PriceTable::load(home.path());
        let (m, f) = PriceTable::paths(home.path());
        assert_eq!(t.manual_path.as_deref(), Some(m.as_path()));
        assert_eq!(t.fetched_path.as_deref(), Some(f.as_path()));
        assert!(!m.exists() && !f.exists(), "load must never seed a file");
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
