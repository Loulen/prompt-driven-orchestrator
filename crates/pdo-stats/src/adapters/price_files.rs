//! The price table's files (ADR-0034): the host home they live under, their
//! read (the run context's `PriceTable::load`), and the atomic write of the
//! fetched tier.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use pdo_run::domain::prices::parse::FETCHED_SCHEMA;
use pdo_run::domain::prices::{Price, PriceTable};

use crate::application::ports::PriceFiles;

/// The price files of this daemon's host home.
pub struct HomePriceFiles {
    /// Replaces `$HOME` (the per-daemon test seam).
    home_override: Option<PathBuf>,
}

impl HomePriceFiles {
    pub fn new(home_override: Option<PathBuf>) -> Self {
        Self { home_override }
    }
}

impl PriceFiles for HomePriceFiles {
    /// Prices are an **instance** concept: even for a sandboxed Run this is
    /// the HOST home — the sandbox seam moves the transcript root, not this
    /// one.
    fn home(&self) -> Result<PathBuf, String> {
        if let Some(home) = &self.home_override {
            return Ok(home.clone());
        }
        pdo_sandbox::adapters::host::default_roots_from_env()
            .map(|(home, _)| home)
            .ok_or_else(|| {
                "cannot resolve the home root for the price files: \
                 HOME is not set; cannot resolve the sandbox staging root"
                    .to_string()
            })
    }

    fn load(&self, home: &Path) -> PriceTable {
        PriceTable::load(home)
    }

    fn paths(&self, home: &Path) -> (PathBuf, PathBuf) {
        PriceTable::paths(home)
    }

    fn write_fetched(
        &self,
        path: &Path,
        source: &str,
        fetched_at: &str,
        rows: &BTreeMap<String, Price>,
    ) -> std::io::Result<()> {
        write_fetched(path, source, fetched_at, rows)
    }
}

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Serialise and write `fetched.json` by tmp + `rename` **in the same
/// directory** (idiom of the sandbox staging's `atomic_copy_into`), so a concurrent
/// `PriceTable::load` can never read a half-written document.
///
/// **Never writes an empty table** — the one path by which this feature could
/// destroy something.
pub fn write_fetched(
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
