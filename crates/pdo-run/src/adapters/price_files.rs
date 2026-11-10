//! The price table's two disk tiers, read from an injected root:
//! `<home_root>/.pdo/prices/{models.yaml, fetched.json}`. Never `$HOME`, never
//! the network.

use std::path::{Path, PathBuf};

use crate::domain::prices::parse::{parse_fetched, parse_manual};
use crate::domain::prices::{fingerprint_of, PriceTable};

impl PriceTable {
    /// `<home_root>/.pdo/prices/{models.yaml, fetched.json}` — path arithmetic
    /// only.
    pub fn paths(home_root: &Path) -> (PathBuf, PathBuf) {
        let dir = home_root.join(".pdo").join("prices");
        (dir.join("models.yaml"), dir.join("fetched.json"))
    }

    /// Load the effective table from an injected root. Absent or unreadable →
    /// that tier is empty and SILENT (the normal state of every instance).
    /// Unparseable, or an unknown `schema` → the tier is ENTIRELY inert plus a
    /// diagnostic. Emits AT MOST ONE `warn!` per distinct fingerprint, so a
    /// polled `/stats/cost` cannot produce one log line per request.
    pub fn load(home_root: &Path) -> Self {
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
}

/// Which price tier won, as `GET /settings` shows it: an observed STATE, not a
/// knob. A hand-edited file passes through NO validator by construction, so the
/// only honest place to surface a refused row is here, as an advisory `reason`
/// (the SAME string the loader logs). Both paths are ALWAYS named, even when
/// neither file exists — nothing is ever seeded (ADR-0031 §2), so naming the
/// paths IS the whole discoverability story.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PriceTableState {
    pub manual_path: Option<String>,
    pub fetched_path: Option<String>,
    pub source: Option<String>,
    pub fetched_at: Option<String>,
    pub fetched_rows: usize,
    pub manual_keys: Vec<String>,
    pub reason: Option<String>,
}

/// The state of the table under `home_root`, or what `None` (HOME unset) means.
pub fn price_table_state(home_root: Option<&Path>) -> PriceTableState {
    let Some(home) = home_root else {
        return PriceTableState {
            manual_path: None,
            fetched_path: None,
            source: None,
            fetched_at: None,
            fetched_rows: 0,
            manual_keys: Vec::new(),
            reason: Some(
                "HOME is unset, so the price files have no resolvable path — \
                 only the prices compiled into the binary apply"
                    .to_string(),
            ),
        };
    };
    let table = PriceTable::load(home);
    let (manual_path, fetched_path) = PriceTable::paths(home);
    PriceTableState {
        manual_path: Some(manual_path.to_string_lossy().into_owned()),
        fetched_path: Some(fetched_path.to_string_lossy().into_owned()),
        source: table.source().map(str::to_string),
        fetched_at: table.fetched_at().map(str::to_string),
        fetched_rows: table.fetched_rows(),
        manual_keys: table.manual_keys().to_vec(),
        reason: table.diagnostic(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::prices::parse::FETCHED_SCHEMA;
    use crate::domain::prices::{fetched_doc, p, PriceTier};

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
        // #527: gen-5 is floored, so a never-synced instance prices its default
        // model out of the box — the whole point of the amendment.
        assert_eq!(t.price_for("claude-opus-5"), Some(p(5.0, 25.0)));
        assert_eq!(t.tier_of("claude-opus-5"), Some(PriceTier::Embedded));
        // A model the floor cannot know yet is still unpriced until a sync.
        assert_eq!(t.price_for("claude-opus-6"), None);
        // The floor is exactly `builtin()` — nothing seeded on disk.
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
        // #527: gen-5 falls back to the embedded floor when the fetched tier is inert.
        assert_eq!(t.price_for("claude-opus-5"), Some(p(5.0, 25.0)));
        // A model beyond the floor stays unpriced.
        assert_eq!(t.price_for("claude-opus-6"), None);
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
}
