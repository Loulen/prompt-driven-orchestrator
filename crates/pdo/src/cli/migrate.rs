//! `pdo migrate` (ADR-0011): rewrite the pipeline YAMLs and node-library entries
//! on disk that still carry a retired shape.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;
use pdo_pipeline::domain::library as node_library;
use pdo_pipeline::domain::migration as pipeline_migrator;
use pdo_run::application::paths::dirs_next_home;

/// One-shot `pdo migrate` (ADR-0011): walks the pipeline stores and rewrites
/// every retired shape via [`pipeline_migrator::migrate_pipeline_yaml`] —
/// legacy control nodes (Switch/Loop/ForEach) dissolved into edges and `loops:`
/// regions, a port's `side` moved onto its edges and its `repeated` dropped.
/// Without `--dir`, it also rewrites the node library's entries. A real
/// migration first copies the original to `<file>.bak`.
pub fn run_migrate(dir: Option<std::path::PathBuf>, dry_run: bool) -> Result<()> {
    let home = dirs_next_home();
    let (pipeline_dirs, library_dir) = match dir {
        Some(d) => (vec![d], None),
        None => {
            let mut v = Vec::new();
            if let Ok(cwd) = std::env::current_dir() {
                v.push(cwd.join(".pdo").join("pipelines"));
            }
            if let Some(home) = &home {
                v.push(home.join(".pdo").join("pipelines"));
            }
            v.retain(|d| d.is_dir());
            v.dedup();
            let library = home
                .map(|h| h.join(".pdo").join("library"))
                .filter(|d| d.is_dir());
            (v, library)
        }
    };
    migrate_stores(
        &pipeline_dirs,
        library_dir.as_deref(),
        dry_run,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )
}

/// The YAML files of `dir`, sorted so the report reads the same on every run.
fn yaml_files(dir: &Path, err: &mut dyn Write, errors: &mut usize) -> Vec<PathBuf> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            let _ = writeln!(err, "skip {}: {e}", dir.display());
            *errors += 1;
            return Vec::new();
        }
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext == "yaml" || ext == "yml")
        })
        .collect();
    files.sort();
    files
}

fn backup(path: &Path) -> std::io::Result<PathBuf> {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let backup = path.with_file_name(format!("{name}.bak"));
    std::fs::copy(path, &backup)?;
    Ok(backup)
}

/// `(what changed)` appended to a report line, when the migration says.
fn details(summary: Option<String>) -> String {
    summary.map(|s| format!(": {s}")).unwrap_or_default()
}

/// Migrates the pipelines of `pipeline_dirs` and the entries of `library_dir`,
/// reporting one line per rewritten file and a closing count on `out`.
pub(crate) fn migrate_stores(
    pipeline_dirs: &[PathBuf],
    library_dir: Option<&Path>,
    dry_run: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<()> {
    if pipeline_dirs.is_empty() && library_dir.is_none() {
        writeln!(out, "no pipeline directory found — nothing to migrate")?;
        return Ok(());
    }

    let mut migrated = 0usize;
    let mut up_to_date = 0usize;
    let mut errors = 0usize;
    let verb = if dry_run { "would migrate" } else { "migrated" };

    for d in pipeline_dirs {
        for path in yaml_files(d, err, &mut errors) {
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) => {
                    writeln!(err, "error   {}: {e}", path.display())?;
                    errors += 1;
                    continue;
                }
            };
            match pipeline_migrator::migrate_pipeline_yaml(&text, &path) {
                Ok(res) if res.migrated => {
                    let details = details(res.ports.summary());
                    if dry_run {
                        writeln!(out, "{verb} {}{details}", path.display())?;
                        migrated += 1;
                        continue;
                    }
                    let backup = match backup(&path) {
                        Ok(b) => b,
                        Err(e) => {
                            writeln!(err, "error   {}: backup failed: {e}", path.display())?;
                            errors += 1;
                            continue;
                        }
                    };
                    match pdo_pipeline::adapters::fs_migration::migrate_pipeline_file(&path) {
                        Ok(_) => {
                            writeln!(
                                out,
                                "{verb} {} (backup: {}){details}",
                                path.display(),
                                backup.display()
                            )?;
                            migrated += 1;
                        }
                        Err(e) => {
                            writeln!(err, "error   {}: {e}", path.display())?;
                            errors += 1;
                        }
                    }
                }
                Ok(_) => up_to_date += 1,
                Err(e) => {
                    writeln!(err, "error   {}: {e}", path.display())?;
                    errors += 1;
                }
            }
        }
    }

    let mut entries_migrated = 0usize;
    if let Some(library) = library_dir {
        for path in yaml_files(library, err, &mut errors) {
            let rewritten = std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|text| node_library::migrate_entry_yaml(&text));
            match rewritten {
                Ok(Some((yaml, report))) => {
                    let details = details(report.summary());
                    if dry_run {
                        writeln!(out, "{verb} library entry {}{details}", path.display())?;
                        entries_migrated += 1;
                        continue;
                    }
                    let written = backup(&path).and_then(|backup| {
                        std::fs::write(&path, yaml)?;
                        Ok(backup)
                    });
                    match written {
                        Ok(backup) => {
                            writeln!(
                                out,
                                "{verb} library entry {} (backup: {}){details}",
                                path.display(),
                                backup.display()
                            )?;
                            entries_migrated += 1;
                        }
                        Err(e) => {
                            writeln!(err, "error   {}: {e}", path.display())?;
                            errors += 1;
                        }
                    }
                }
                Ok(None) => up_to_date += 1,
                Err(e) => {
                    writeln!(err, "error   {}: {e}", path.display())?;
                    errors += 1;
                }
            }
        }
    }

    writeln!(
        out,
        "{verb} {migrated} pipeline(s) and {entries_migrated} library entry(ies); {up_to_date} already up to date; {errors} error(s)"
    )?;
    if errors > 0 {
        anyhow::bail!("{errors} file(s) failed to migrate");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEGACY_PIPELINE: &str = r#"name: legacy
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - { name: user_prompt, side: right }
  - id: aBcD1234
    name: worker
    type: agent
    isolated_worktree: false
    outputs:
      - { name: plan, side: bottom, repeated: true }
  - id: end
    name: End
    type: end
    inputs:
      - { name: result, side: top }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: aBcD1234, port: in }
  - source: { node: aBcD1234, port: plan }
    target: { node: end, port: result }
    repeated: true
"#;

    const LEGACY_ENTRY: &str =
        "name: Reviewer\ntype: agent\noutputs:\n- name: review\n  repeated: true\n  side: bottom\nprompt: You review.\n";

    fn run(pipelines: &Path, library: &Path, dry_run: bool) -> String {
        let mut out = Vec::new();
        let mut err = Vec::new();
        migrate_stores(
            &[pipelines.to_path_buf()],
            Some(library),
            dry_run,
            &mut out,
            &mut err,
        )
        .unwrap();
        assert!(err.is_empty(), "{}", String::from_utf8_lossy(&err));
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn migrate_rewrites_the_registry_and_the_library_and_says_what_changed() {
        let tmp = tempfile::tempdir().unwrap();
        let pipelines = tmp.path().join("pipelines");
        let library = tmp.path().join("library");
        std::fs::create_dir_all(&pipelines).unwrap();
        std::fs::create_dir_all(&library).unwrap();
        let pipeline = pipelines.join("legacy.yaml");
        let entry = library.join("reviewer.yaml");
        std::fs::write(&pipeline, LEGACY_PIPELINE).unwrap();
        std::fs::write(&entry, LEGACY_ENTRY).unwrap();

        // A dry run reports and writes nothing.
        let report = run(&pipelines, &library, true);
        assert!(
            report.contains("would migrate 1 pipeline(s) and 1 library entry(ies)"),
            "{report}"
        );
        assert_eq!(std::fs::read_to_string(&pipeline).unwrap(), LEGACY_PIPELINE);

        let report = run(&pipelines, &library, false);
        assert!(
            report.contains(
                "legacy.yaml.bak): 3 port side(s) removed, 1 source_anchor(s) added, 1 target_side(s) added, 1 port repeated removed"
            ),
            "{report}"
        );
        assert!(
            report.contains("reviewer.yaml.bak): 1 port side(s) removed, 1 port repeated removed"),
            "{report}"
        );
        assert!(
            report
                .contains("migrated 1 pipeline(s) and 1 library entry(ies); 0 already up to date"),
            "{report}"
        );

        let migrated: serde_yaml::Value =
            serde_yaml::from_str(&std::fs::read_to_string(&pipeline).unwrap()).unwrap();
        for node in migrated["nodes"].as_sequence().unwrap() {
            for key in ["inputs", "outputs"] {
                for port in node[key].as_sequence().into_iter().flatten() {
                    assert!(port.get("side").is_none() && port.get("repeated").is_none());
                }
            }
        }
        let edge = &migrated["edges"][1];
        assert_eq!(edge["source_anchor"]["side"].as_str(), Some("bottom"));
        assert!(edge["source_anchor"].get("offset").is_none());
        assert_eq!(edge["target_side"].as_str(), Some("top"));
        assert_eq!(edge["repeated"].as_bool(), Some(true));
        let entry_text = std::fs::read_to_string(&entry).unwrap();
        assert!(!entry_text.contains("side") && !entry_text.contains("repeated"));

        // Idempotent: a second run finds nothing to do and touches nothing.
        let pipeline_after = std::fs::read_to_string(&pipeline).unwrap();
        let report = run(&pipelines, &library, false);
        assert!(
            report
                .contains("migrated 0 pipeline(s) and 0 library entry(ies); 2 already up to date"),
            "{report}"
        );
        assert_eq!(std::fs::read_to_string(&pipeline).unwrap(), pipeline_after);
        assert_eq!(std::fs::read_to_string(&entry).unwrap(), entry_text);
    }
}
