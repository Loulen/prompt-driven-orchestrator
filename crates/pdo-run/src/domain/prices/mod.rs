//! The price table in force for ONE cost read: three tiers merged **by family
//! key** — manual (`~/.pdo/prices/models.yaml`, written by the human alone) →
//! fetched (`~/.pdo/prices/fetched.json`, written by the daemon alone) →
//! embedded (`builtin::PRICES`, the compiled floor). See ADR-0034 and ADR-0022's #427
//! amendment.
//!
//! This module never reads `$HOME`: a root goes in, path-math + `std::fs` come
//! out. A global `$HOME` read costs a crate-wide test lock (see
//! `library_store.rs:967`).
//!
//! ## What is load-bearing here
//! - **Merge by key, never replacement.** A key present in a tier wins; a key
//!   absent keeps whatever the next tier says. Under global replacement,
//!   forgetting `claude-opus-4-8` would erase 79 941 of ~116 200 transcript
//!   lines — a wrong-price bug converted into a total blackout — and would
//!   freeze the table against future releases.
//! - **The embedded tier is a FLOOR, not a seed.** `claude-opus-4-0`,
//!   `claude-sonnet-4-0` and `claude-3-5-haiku` are absent from every remote
//!   source examined, so the `const` is their ONLY pricer. The floor ALSO carries
//!   the current generation, so a never-synced offline instance prices its
//!   default model instead of reading `~$0.0000 †`; models.dev does carry those,
//!   so a sync overrides them by key. "Seed" in ADR-0034 means materialising the
//!   `const` onto a disk file; adding `const` rows is not that.
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
//! The tiers are read from disk by `adapters::price_files` ([`PriceTable::load`]);
//! the sync that writes the fetched tier (the single egress) lives with the
//! settings of the instance, strictly OUTSIDE the read path.


mod builtin;
pub mod parse;

use std::collections::BTreeMap;
use std::hash::Hasher;
use std::path::PathBuf;
use std::sync::Mutex;
use tracing::warn;
use ts_rs::TS;

use builtin::{PRICES, SYNTHETIC};

/// Drop a trailing 8-digit date segment so a dated id resolves to its family
/// key: `claude-sonnet-4-5-20250929` → `claude-sonnet-4-5`. A version-only id is
/// returned unchanged. Moved here from `run_cost.rs` — the fetch normaliser uses
/// it too.
pub fn strip_date_suffix(model: &str) -> &str {
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
pub struct Price {
    pub input: f64,
    pub output: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum PriceTier {
    Manual,
    Fetched,
    Embedded,
}

/// A refused row and why — the material of BOTH the `warn!` and the
/// `GET /settings` `reason`, so the two can never drift.
#[derive(Debug, Clone, PartialEq)]
pub struct RejectedRow {
    pub key: String,
    pub why: String,
}

/// Where a fetched document came from and when. `None` for the manual tier: the
/// human's file carries no provenance, by design (PDO never writes it).
#[derive(Debug, Clone, PartialEq)]
pub struct Provenance {
    pub source: String,
    pub fetched_at: String,
}

/// One parsed on-disk tier. An unparseable document yields `unparseable:
/// Some(err)` and NO rows — never an `Err`: a bad price file must not be able to
/// fail a cost read.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ParsedTier {
    pub rows: BTreeMap<String, Price>,
    pub rejected: Vec<RejectedRow>,
    pub unparseable: Option<String>,
    pub provenance: Option<Provenance>,
}

impl ParsedTier {
    #[cfg(test)]
    pub(crate) fn of(rows: &[(&str, f64, f64)]) -> Self {
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
/// is. That fingerprint is why this is not a bare map: it is a component of
/// `run_cost`'s memo key, without which a sync would stay invisible on
/// `/stats/cost` until the daemon restarted.
#[derive(Debug, Clone, PartialEq)]
pub struct PriceTable {
    pub(crate) resolved: BTreeMap<String, (Price, PriceTier)>,
    pub(crate) manual: ParsedTier,
    pub(crate) fetched: ParsedTier,
    pub(crate) manual_keys: Vec<String>,
    pub(crate) fingerprint: u64,
    /// Set by [`Self::load`] only, so `diagnostic()` can name the real file.
    /// `None` for a table built by the pure [`Self::resolve`].
    pub(crate) manual_path: Option<PathBuf>,
    pub(crate) fetched_path: Option<PathBuf>,
}

impl Default for PriceTable {
    fn default() -> Self {
        Self::builtin()
    }
}

impl PriceTable {
    /// The floor alone. Pure, no IO, no `$HOME`. `fingerprint() == 0`.
    pub fn builtin() -> Self {
        Self::resolve(ParsedTier::default(), ParsedTier::default(), 0)
    }

    /// PURE, TOTAL resolver — tiers injected, zero IO. ONE resolver shared by
    /// the cost computation and by the `GET /settings` view, so what the UI
    /// shows can never drift from what actually prices.
    pub fn resolve(manual: ParsedTier, fetched: ParsedTier, fingerprint: u64) -> Self {
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


    /// Per-MTok price, or `None` for a model unknown to all THREE tiers (the
    /// caller then flips `partial` and the line contributes $0).
    ///
    /// `<synthetic>` is priced $0 **above every table** — ORDER IS LOAD-BEARING.
    /// Moving the lookup first would let a price file turn the sentinel into a
    /// real cost, and would break the `Some((0,0))` / `None` distinction that
    /// keeps `partial` honest.
    pub fn price_for(&self, model: &str) -> Option<Price> {
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
    pub fn tier_of(&self, key: &str) -> Option<PriceTier> {
        self.resolved.get(key).map(|(_, t)| *t)
    }

    /// The resolved table, in `BTreeMap` order. The SAME map `price_for` reads,
    /// so the `GET /settings` view can never enumerate a set the pricer would
    /// price otherwise. Exposes neither the container nor the internal
    /// `(Price, PriceTier)` tuple, so the wire JSON stays assembled in axum.
    pub fn resolved_entries(&self) -> impl Iterator<Item = (&str, Price, PriceTier)> + '_ {
        self.resolved.iter().map(|(k, (p, t))| (k.as_str(), *p, *t))
    }

    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }

    /// The keys the manual tier actually decided — the `GET /settings` signal
    /// that a hand edit is shadowing a sync.
    pub fn manual_keys(&self) -> &[String] {
        &self.manual_keys
    }

    /// Rows accepted from `fetched.json` (0 when the file is absent or inert).
    pub fn fetched_rows(&self) -> usize {
        self.fetched.rows.len()
    }

    /// The fetched tier's rows, for the sync's added/updated/unchanged diff.
    pub fn fetched_prices(&self) -> &BTreeMap<String, Price> {
        &self.fetched.rows
    }

    /// Vintage of the fetched tier: the table's date is readable, not guessed.
    pub fn fetched_at(&self) -> Option<&str> {
        self.fetched
            .provenance
            .as_ref()
            .map(|p| p.fetched_at.as_str())
    }

    /// URL of the last successful fetch.
    pub fn source(&self) -> Option<&str> {
        self.fetched.provenance.as_ref().map(|p| p.source.as_str())
    }

    /// ONE message naming every inert file and every refused row, or `None` when
    /// nothing is wrong. PURE, so a unit test can pin it instead of a terminal.
    /// ADR-0015:44 — two lines for one problem read as two problems.
    pub fn diagnostic(&self) -> Option<String> {
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
    pub(crate) fn warn_once(&self) {
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
/// Preferred over mtime: no millisecond granularity, no copy-preserves-mtime
/// trap. The hasher need NOT be stable across Rust versions — the only consumer
/// is `run_cost`'s process-local RAM memo.
pub fn fingerprint_of(manual: Option<&[u8]>, fetched: Option<&[u8]>) -> u64 {
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


/// A price, for the tests of the tiers.
#[cfg(test)]
pub(crate) fn p(input: f64, output: f64) -> Price {
    Price { input, output }
}

/// A `fetched.json` under `schema`, pricing one family.
#[cfg(test)]
pub(crate) fn fetched_doc(schema: &str) -> String {
    format!(
        r#"{{"schema":"{schema}","source":"https://models.dev/api.json","fetched_at":"2026-07-30T14:12:03Z","models":{{"claude-opus-5":{{"input":5.0,"output":25.0}}}}}}"#
    )
}

#[cfg(test)]
mod tests {
    use super::parse::*;
    use super::*;

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
    fn gen_5_is_priced_by_the_embedded_floor_out_of_the_box() {
        // #527: the offline, never-synced default. `sonnet-5` is the post-intro
        // $3/$15, not the dated intro $2/$10 — the `const` cannot be dated.
        let t = PriceTable::builtin();
        assert_eq!(t.price_for("claude-opus-5"), Some(p(5.0, 25.0)));
        assert_eq!(t.price_for("claude-sonnet-5"), Some(p(3.0, 15.0)));
        assert_eq!(t.price_for("claude-fable-5"), Some(p(10.0, 50.0)));
        // A dated gen-5 id resolves to its family price the same way.
        assert_eq!(
            t.price_for("claude-opus-5-20260723"),
            Some(p(5.0, 25.0)),
            "a dated gen-5 id must strip to its family"
        );
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
        // A model NEWER than the floor knows (`claude-opus-6`): the enduring value
        // of the fetched tier once #527 floored the current generation.
        let t = PriceTable::resolve(
            ParsedTier::default(),
            ParsedTier::of(&[("claude-opus-6", 10.0, 50.0)]),
            7,
        );
        assert_eq!(t.price_for("claude-opus-6"), Some(p(10.0, 50.0)));
        assert_eq!(t.tier_of("claude-opus-6"), Some(PriceTier::Fetched));
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
        // #527: the gen-5 floor is the OPPOSITE case — models.dev carries these, so a
        // sync overrides them by key. The live intro `sonnet-5` $2/$10 wins over the
        // floor's post-intro $3/$15. That is what makes it a floor, not a mirror.
        assert_eq!(t.price_for("claude-sonnet-5"), Some(p(2.0, 10.0)));
        assert_eq!(t.tier_of("claude-sonnet-5"), Some(PriceTier::Fetched));
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
    fn resolved_entries_on_builtin_are_the_fourteen_embedded_families() {
        let rows: Vec<(String, Price, PriceTier)> = PriceTable::builtin()
            .resolved_entries()
            .map(|(k, p, t)| (k.to_string(), p, t))
            .collect();
        // Exactly the floor, no more, no less — fourteen families since #527
        // seeded the current generation into the embedded floor.
        assert_eq!(rows.len(), PRICES.len());
        assert_eq!(rows.len(), 14);
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
            ParsedTier::of(&[("claude-opus-6", 10.0, 50.0)]),
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
        // Fetch-only family (beyond the #527 floor, so only the fetched tier
        // knows it): Fetched tier.
        assert_eq!(
            row("claude-opus-6"),
            Some((
                "claude-opus-6".to_string(),
                p(10.0, 50.0),
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

}
