//! `pdo migrate` (ADR-0011): dissolve the retired control nodes of the pipeline
//! YAMLs on disk into the `loops:` region model.

use std::path::PathBuf;

use anyhow::Result;
use pdo_pipeline::domain::migration as pipeline_migrator;
use pdo_run::application::paths::dirs_next_home;

/// One-shot `pdo migrate` (ADR-0011): walks the pipeline stores and dissolves
/// legacy control-node YAML (Switch/Loop/ForEach) into the edge-conditional +
/// `loops:` region model via [`pipeline_migrator::migrate_pipeline_yaml`]. A real
/// migration first copies the original to `<file>.bak`.
pub fn run_migrate(dir: Option<std::path::PathBuf>, dry_run: bool) -> Result<()> {
    let dirs: Vec<PathBuf> = match dir {
        Some(d) => vec![d],
        None => {
            let mut v = Vec::new();
            if let Ok(cwd) = std::env::current_dir() {
                v.push(cwd.join(".pdo").join("pipelines"));
            }
            if let Some(home) = dirs_next_home() {
                v.push(home.join(".pdo").join("pipelines"));
            }
            v.retain(|d| d.is_dir());
            v
        }
    };

    if dirs.is_empty() {
        println!("no pipeline directory found — nothing to migrate");
        return Ok(());
    }

    let mut migrated = 0usize;
    let mut up_to_date = 0usize;
    let mut errors = 0usize;

    for d in &dirs {
        let entries = match std::fs::read_dir(d) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("skip {}: {e}", d.display());
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_yaml = path
                .extension()
                .is_some_and(|ext| ext == "yaml" || ext == "yml");
            if !is_yaml {
                continue;
            }
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("error   {}: {e}", path.display());
                    errors += 1;
                    continue;
                }
            };
            match pipeline_migrator::migrate_pipeline_yaml(&text, &path) {
                Ok(res) if res.migrated => {
                    if dry_run {
                        println!("would migrate {}", path.display());
                        migrated += 1;
                    } else {
                        let backup =
                            path.with_file_name(format!("{}.bak", entry.file_name().display()));
                        if let Err(e) = std::fs::copy(&path, &backup) {
                            eprintln!("error   {}: backup failed: {e}", path.display());
                            errors += 1;
                            continue;
                        }
                        match pdo_pipeline::adapters::fs_migration::migrate_pipeline_file(&path) {
                            Ok(_) => {
                                println!(
                                    "migrated {} (backup: {})",
                                    path.display(),
                                    backup.display()
                                );
                                migrated += 1;
                            }
                            Err(e) => {
                                eprintln!("error   {}: {e}", path.display());
                                errors += 1;
                            }
                        }
                    }
                }
                Ok(_) => up_to_date += 1,
                Err(e) => {
                    eprintln!("error   {}: {e}", path.display());
                    errors += 1;
                }
            }
        }
    }

    let verb = if dry_run { "would migrate" } else { "migrated" };
    println!("{verb} {migrated} pipeline(s); {up_to_date} already up to date; {errors} error(s)");
    if errors > 0 {
        anyhow::bail!("{errors} pipeline(s) failed to migrate");
    }
    Ok(())
}
