//! The wire views of a staging profile, byte for byte as the editor has always
//! read them (`GET|PUT /settings/sandbox-profiles/{name}`).

use std::path::Path;

use crate::domain::container::RUN_CONSTANT_ENV_KEYS;
use crate::domain::profile::{
    base_entries, DefaultEntry, EntryKind, ResolvedProfile, DEFAULT_FULL_ENTRIES,
    SENSITIVE_PREFIXES, STAGING_GUARANTEES,
};

/// Serialise one resolved entry for the editor.
///
/// Advisories are computed **server-side** deliberately: deriving the size warning or
/// the sensitivity flag in the client would re-open the drift this codebase has paid
/// for before, and the size figure is a measurement recorded with the default entries,
/// not something a browser can know. `exists` is one `symlink_metadata` per entry.
fn entry_view(
    home_root: Option<&Path>,
    path: &str,
    default_entry: Option<&DefaultEntry>,
    enabled: bool,
) -> serde_json::Value {
    let sensitive = SENSITIVE_PREFIXES
        .iter()
        .any(|p| path == *p || path.starts_with(&format!("{p}/")));
    let is_glob = default_entry.is_some_and(|d| d.kind == EntryKind::Glob);
    let on_disk = match (home_root, is_glob) {
        (Some(home), false) => Some(home.join(path)),
        _ => None,
    };
    let exists = on_disk
        .as_ref()
        .map(|p| std::fs::symlink_metadata(p).is_ok());
    let kind = match default_entry.map(|d| d.kind) {
        Some(k) => k,
        // An extra's kind is an OBSERVED fact: the stored diff does not carry it, and
        // re-deriving it here keeps the row honest after a `rm -rf`.
        None => match on_disk.as_ref().map(|p| p.is_dir()) {
            Some(true) => EntryKind::Dir,
            _ => EntryKind::File,
        },
    };
    serde_json::json!({
        "path": path,
        "kind": kind,
        "from_default": default_entry.is_some(),
        "enabled": enabled,
        "resynthesised": default_entry.is_some_and(|d| d.resynthesised),
        "note": default_entry.and_then(|d| d.note),
        "sensitive": sensitive,
        "exists": exists,
    })
}

/// The full editor view of one resolved profile.
pub(crate) fn profile_view(
    home_root: Option<&Path>,
    profile: &ResolvedProfile,
) -> serde_json::Value {
    let base = base_entries(&profile.name);
    // Default entries first, in the constant's own reading order — NOT sorted, or the
    // editor's checklist reorders on every edit.
    let mut entries: Vec<serde_json::Value> = DEFAULT_FULL_ENTRIES
        .iter()
        .filter(|d| base.contains(&d.path))
        .map(|d| {
            let enabled = profile.resolved.entries.iter().any(|e| e == d.path);
            entry_view(home_root, d.path, Some(d), enabled)
        })
        .collect();
    for extra in &profile.extras {
        let enabled = profile.resolved.entries.iter().any(|e| e == extra);
        entries.push(entry_view(home_root, extra, None, enabled));
    }
    let floor: Vec<serde_json::Value> = STAGING_GUARANTEES
        .iter()
        .map(|g| serde_json::json!({ "id": g.id, "label": g.label, "path": g.path }))
        .collect();
    serde_json::json!({
        "name": profile.name,
        "virtual": profile.is_virtual,
        "materialised": profile.materialised,
        "disabled": profile.disabled,
        "extras": profile.extras,
        // Exactly what a Run created now would stage.
        "resolved": profile.resolved.entries,
        "entries": entries,
        // Signalled no-ops, never errors (ADR-0031 §2): the default may lose an entry
        // tomorrow, and a `disabled` for an entry this version lacks must be remembered.
        "redundant_extras": profile.resolved.redundant_extras,
        "inactive_disabled": profile.resolved.inactive_disabled,
        "floor": floor,
        "sensitive_prefixes": SENSITIVE_PREFIXES,
        // The env posed at `docker create`, VALUES INCLUDED. This is not a secret store
        // and the UI says so: the values already sit in clear in SQLite, in the Run's
        // frozen `run_started` payload and in `docker inspect`. Masking them here would
        // be theatre that let someone believe PDO is protecting them. The one place
        // they must NOT appear is the daemon log (`env_names`),
        // because the journal outlives the Run and the profile.
        "env": profile.env,
        // The keys PDO poses itself and therefore refuses. Server-owned, so the editor
        // greys them out instead of hard-coding a parallel list that would drift the
        // day a fourth run-constant appears.
        "reserved_env_keys": RUN_CONSTANT_ENV_KEYS,
        // Where this profile's container image comes from, or `null` when it poses
        // nothing and the profile default decides. `null` and absent are the same
        // answer here; the editor renders both as "default".
        "image": profile.image,
        "updated_at": profile.updated_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::profile::{resolve_profile, MINIMAL_PROFILE};

    #[test]
    fn the_minimal_view_shows_the_floor_and_no_checklist() {
        let minimal = resolve_profile(MINIMAL_PROFILE, None).unwrap();
        let view = profile_view(None, &minimal);
        assert_eq!(view["name"], "minimal");
        assert_eq!(view["virtual"], true);
        assert_eq!(view["materialised"], false);
        assert_eq!(view["entries"], serde_json::json!([]));
        assert_eq!(
            view["floor"].as_array().unwrap().len(),
            STAGING_GUARANTEES.len()
        );
        assert_eq!(
            view["reserved_env_keys"],
            serde_json::json!(["HOME", "PDO_DAEMON_URL", "PDO_RUN_ID"])
        );
        assert_eq!(view["image"], serde_json::Value::Null);
    }

    #[test]
    fn an_entry_reports_what_is_on_disk_under_the_home() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".config/gh")).unwrap();
        let dir = entry_view(Some(home.path()), ".config/gh", None, true);
        assert_eq!(dir["kind"], "dir");
        assert_eq!(dir["exists"], true);
        assert_eq!(dir["from_default"], false);
        let gone = entry_view(Some(home.path()), ".ssh/id", None, true);
        assert_eq!(gone["exists"], false);
        assert_eq!(gone["sensitive"], true);
        let glob = entry_view(
            Some(home.path()),
            ".claude/*.md",
            DEFAULT_FULL_ENTRIES.get(1),
            true,
        );
        assert_eq!(glob["exists"], serde_json::Value::Null);
    }
}
