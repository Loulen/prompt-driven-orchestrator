//! What the platform reads from the process environment: the `env` tier of the
//! `update_check` setting, the config home, the `node` directory.

use std::path::PathBuf;

use crate::domain::update_check::{parse_bool_setting, UPDATE_CHECK_ENV};

/// The `env` tier of the `update_check` setting (`PDO_UPDATE_CHECK`), read fresh.
pub fn env_update_check() -> Option<bool> {
    std::env::var(UPDATE_CHECK_ENV)
        .ok()
        .as_deref()
        .and_then(parse_bool_setting)
}

/// Resolve `$XDG_CONFIG_HOME`, falling back to `$HOME/.config` (#156, D4).
///
/// Impure (reads env); the pure `systemd_unit_path` takes the result as a
/// parameter so tests stay hermetic. Returns `None` only if neither
/// `XDG_CONFIG_HOME` nor `HOME` is set (a broken environment).
pub fn resolve_config_home() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg));
        }
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"))
}

/// Locate the directory containing the `node` executable by scanning `$PATH`
/// (#156, D3) — the dependency-free analog of the Makefile's
/// `dirname $(command -v node)`. Impure (reads `$PATH`, stats files); returns
/// `None` when node is not on `$PATH`, so callers can warn instead of silently
/// shipping a unit whose daemon can't spawn Claude.
pub fn resolve_node_dir() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join("node");
        if candidate.is_file() {
            return Some(dir);
        }
    }
    None
}
