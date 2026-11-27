//! The staging's markers (CORE-343): the completion marker [`prepare`](super::prepare)
//! writes last, and the staging-set markers a re-prepare invalidates.

use std::path::Path;

use anyhow::{Context, Result};

use crate::domain::staging::{staging_prepared_marker, staging_sets_dir};

/// Efface les marqueurs de staging set (`<staging>/sets`) : jamais de contenu stagé.
/// Absent = no-op ; tout autre échec est fatal, car un `sets/claude` survivant aux
/// fixups écrasés par le walk ferait pendre la session claude suivante sur le
/// dialogue de confiance.
pub(super) fn drop_set_markers(sandbox_root: &Path, run_id: &str) -> Result<()> {
    let sets = staging_sets_dir(sandbox_root, run_id);
    match std::fs::remove_dir_all(&sets) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => {
            Err(e).with_context(|| format!("drop the staging set markers {}", sets.display()))
        }
    }
}

/// Écrit [`staging_prepared_marker`] atomiquement : sa présence implique un walk fini.
/// Contenu = la liste gelée, une entrée par ligne (diagnostic seulement). Le fichier
/// temporaire est **unique par appel** : deux `ensure_ready` concurrents du même Run
/// (création + ouverture du run shell) ne se volent pas leur `rename`.
pub(super) fn write_prepared_marker(
    sandbox_root: &Path,
    run_id: &str,
    entries: &[String],
) -> Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let marker = staging_prepared_marker(sandbox_root, run_id);
    let tmp = marker.with_extension(format!(
        "{}.{}.tmp",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let body: String = entries.iter().map(|e| format!("{e}\n")).collect();
    std::fs::write(&tmp, body)
        .with_context(|| format!("write staging completion marker {}", tmp.display()))?;
    if let Err(e) = std::fs::rename(&tmp, &marker) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e)
            .with_context(|| format!("commit staging completion marker {}", marker.display()));
    }
    Ok(())
}
