//! The modification times the watcher compares, so a touch that changes
//! nothing announces nothing.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;

pub(super) fn seed_mtimes(map: &Mutex<HashMap<PathBuf, SystemTime>>, dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut guard = match map.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str());
        if path.is_file() && ext == Some("yaml") {
            if let Ok(t) = entry.metadata().and_then(|m| m.modified()) {
                guard.insert(path, t);
            }
        } else if path.is_dir()
            && path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".prompts"))
        {
            if let Ok(prompts) = std::fs::read_dir(&path) {
                for p in prompts.flatten() {
                    let pp = p.path();
                    if pp.extension().and_then(|e| e.to_str()) == Some("md") {
                        if let Ok(t) = p.metadata().and_then(|m| m.modified()) {
                            guard.insert(pp, t);
                        }
                    }
                }
            }
        }
    }
}

pub(super) fn seed_run_mtimes(
    map: &Mutex<HashMap<PathBuf, SystemTime>>,
    runs_dir: &std::path::Path,
) {
    let Ok(entries) = std::fs::read_dir(runs_dir) else {
        return;
    };
    let mut guard = match map.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    for entry in entries.flatten() {
        let run_dir = entry.path();
        if !run_dir.is_dir() {
            continue;
        }
        let yaml_path = run_dir.join("pipeline.yaml");
        if yaml_path.is_file() {
            if let Ok(t) = std::fs::metadata(&yaml_path).and_then(|m| m.modified()) {
                guard.insert(yaml_path, t);
            }
        }
        let prompts_dir = run_dir.join("pipeline.prompts");
        if prompts_dir.is_dir() {
            if let Ok(prompts) = std::fs::read_dir(&prompts_dir) {
                for p in prompts.flatten() {
                    let pp = p.path();
                    if pp.extension().and_then(|e| e.to_str()) == Some("md") {
                        if let Ok(t) = p.metadata().and_then(|m| m.modified()) {
                            guard.insert(pp, t);
                        }
                    }
                }
            }
        }
    }
}

pub(super) fn content_actually_changed(
    map: &Mutex<HashMap<PathBuf, SystemTime>>,
    path: &std::path::Path,
) -> bool {
    let current = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let mut guard = match map.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    match current {
        Some(now) => {
            let prev = guard.get(path).copied();
            if prev == Some(now) {
                false
            } else {
                guard.insert(path.to_path_buf(), now);
                true
            }
        }
        None => {
            guard.remove(path);
            true
        }
    }
}
