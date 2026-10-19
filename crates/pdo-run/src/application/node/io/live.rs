//! A node's live artifacts as the node panel reads them (ADR-0089): the
//! declared ones, produced or not, then whatever else the node published in the
//! `live/` folder of its iteration.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Serialize;
use ts_rs::TS;

use pdo_pipeline::domain::pipeline::{self, LiveArtifact, NodeDef, PortType};

use super::*;

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct LiveArtifactIO {
    pub name: String,
    pub port_type: PortType,
    /// Declared on the node; `false` for one it published freely.
    pub declared: bool,
    /// The file or the folder (ending in `/`), relative to the artifacts dir:
    /// where the artifact is, or would be.
    pub path: String,
    /// Its files on disk; empty when it is not produced.
    pub files: Vec<FileInfo>,
    /// What its frontmatter misses of the declared schema, one per field. A
    /// warning, never a refusal.
    pub schema_warnings: Vec<SchemaWarning>,
    /// When it was last written (RFC 3339), `None` when it is not produced.
    pub updated_at: Option<String>,
}

/// A field of a live artifact's frontmatter that misses its declared schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct SchemaWarning {
    /// The field, or `(frontmatter)` when the frontmatter cannot be parsed.
    pub field: String,
    pub reason: String,
}

/// The live artifacts of `node` at `iter`.
pub(super) fn resolve_live(
    node: &NodeDef,
    artifacts_dir: &Path,
    iter: i64,
    not_before: Option<chrono::DateTime<chrono::Utc>>,
    tracked: &HashSet<PathBuf>,
) -> Vec<LiveArtifactIO> {
    let live_dir = crate::domain::blackboard::live_dir(artifacts_dir, &node.id, iter);
    let mut claimed: HashSet<String> = HashSet::new();
    let mut live = Vec::new();

    for declared in &node.live_artifacts {
        let rel = declared.relative_path();
        claimed.insert(rel.trim_end_matches('/').to_string());
        let path = live_dir.join(rel.trim_end_matches('/'));
        live.push(artifact_io(
            artifacts_dir,
            &path,
            declared.name.clone(),
            declared.port_type,
            true,
            Some(declared),
            not_before,
            tracked,
        ));
    }

    let Ok(entries) = std::fs::read_dir(&live_dir) else {
        return live;
    };
    let mut free: Vec<(String, PathBuf, bool)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let file_name = entry.file_name().to_string_lossy().to_string();
            let is_dir = entry.file_type().ok()?.is_dir();
            (!claimed.contains(&file_name) && !is_scratch(&file_name))
                .then(|| (file_name, entry.path(), is_dir))
        })
        .collect();
    free.sort_by(|a, b| a.0.cmp(&b.0));
    for (file_name, path, is_dir) in free {
        let (name, port_type) = if is_dir {
            (file_name, PortType::ImageList)
        } else {
            free_file_kind(&file_name)
        };
        live.push(artifact_io(
            artifacts_dir,
            &path,
            name,
            port_type,
            false,
            None,
            not_before,
            tracked,
        ));
    }
    live
}

/// A file an agent writes before renaming it onto its target (the preamble
/// asks for it), or a hidden one: never an artifact of its own.
fn is_scratch(file_name: &str) -> bool {
    file_name.starts_with('.') || file_name.ends_with(".tmp") || file_name.ends_with('~')
}

/// What a freely published file is, from its extension: named after its stem.
fn free_file_kind(file_name: &str) -> (String, PortType) {
    let path = Path::new(file_name);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| file_name.to_string());
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let port_type = match ext.as_str() {
        "html" | "htm" => PortType::Html,
        _ if pipeline::is_image_file(path) => PortType::Image,
        _ => PortType::Markdown,
    };
    (stem, port_type)
}

#[allow(clippy::too_many_arguments)]
fn artifact_io(
    artifacts_dir: &Path,
    path: &Path,
    name: String,
    port_type: PortType,
    declared: bool,
    declaration: Option<&LiveArtifact>,
    not_before: Option<chrono::DateTime<chrono::Utc>>,
    tracked: &HashSet<PathBuf>,
) -> LiveArtifactIO {
    let is_folder = matches!(port_type, PortType::Image | PortType::ImageList) && !path.is_file();
    let mut files = if is_folder && declared {
        list_image_files(artifacts_dir, path)
    } else if is_folder {
        // A folder nobody declared: whatever it holds is the artifact.
        list_folder_files(artifacts_dir, path)
    } else if path.is_file() {
        vec![if port_type == PortType::Markdown {
            file_info_with_frontmatter(artifacts_dir, path)
        } else {
            file_info(artifacts_dir, path)
        }]
    } else {
        Vec::new()
    };
    mark_inherited(artifacts_dir, &mut files, not_before, tracked);

    let schema_warnings = match (declaration.and_then(|d| d.frontmatter.as_ref()), &files[..]) {
        (Some(schema), [file]) if !schema.is_empty() && port_type == PortType::Markdown => {
            schema_warnings(&name, schema, &artifacts_dir.join(&file.path))
        }
        _ => Vec::new(),
    };

    let updated_at = files
        .iter()
        .filter_map(|f| {
            std::fs::metadata(artifacts_dir.join(&f.path))
                .and_then(|m| m.modified())
                .ok()
        })
        .max()
        .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());

    let mut rel = relative_path(artifacts_dir, path);
    if is_folder {
        rel.push('/');
    }
    LiveArtifactIO {
        name,
        port_type,
        declared,
        path: rel,
        files,
        schema_warnings,
        updated_at,
    }
}

/// Every file of a freely published folder, sorted, scratch files aside.
fn list_folder_files(artifacts_dir: &Path, dir: &Path) -> Vec<FileInfo> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<FileInfo> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().ok().is_some_and(|ft| ft.is_file()))
        .filter(|e| !is_scratch(&e.file_name().to_string_lossy()))
        .map(|e| file_info(artifacts_dir, &e.path()))
        .collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

/// The completion's frontmatter rule, read as warnings (ADR-0089).
fn schema_warnings(
    name: &str,
    schema: &HashMap<String, pipeline::FrontmatterFieldDecl>,
    path: &Path,
) -> Vec<SchemaWarning> {
    let fields = match pdo_pipeline::adapters::frontmatter_file::parse_frontmatter_from_file(path) {
        Ok(fields) => fields,
        Err(_) => {
            return vec![SchemaWarning {
                field: "(frontmatter)".into(),
                reason: "frontmatter could not be parsed".into(),
            }]
        }
    };
    let mut warnings: Vec<SchemaWarning> =
        crate::application::node::outputs::frontmatter_violations(name, schema, &fields)
            .into_iter()
            .map(|v| SchemaWarning {
                field: v.field,
                reason: v.reason,
            })
            .collect();
    warnings.sort_by(|a, b| a.field.cmp(&b.field));
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdo_pipeline::domain::pipeline::{FrontmatterFieldDecl, NodeType};
    use pretty_assertions::assert_eq;
    use std::fs;

    fn node(live_artifacts: Vec<LiveArtifact>, free: bool) -> NodeDef {
        NodeDef {
            free_live_artifacts: free,
            live_artifacts,
            skills: Vec::new(),
            isolated_worktree: None,
            id: "designer".into(),
            name: "designer".into(),
            node_type: NodeType::Agent,
            inputs: Vec::new(),
            outputs: Vec::new(),
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
            pin_harness: None,
            harnesses: Default::default(),
            agent_choice: None,
            auto_fail: None,
            orchestrator: false,
            on_demand: false,
        }
    }

    fn declared(name: &str, port_type: PortType) -> LiveArtifact {
        LiveArtifact {
            name: name.into(),
            port_type,
            instructions: None,
            frontmatter: None,
        }
    }

    fn progress_with_schema() -> LiveArtifact {
        let mut progress = declared("progress", PortType::Markdown);
        progress.frontmatter = Some(
            [
                (
                    "status".to_string(),
                    FrontmatterFieldDecl {
                        field_type: "enum".into(),
                        allowed: Some(vec!["drafting".into(), "ready".into()]),
                    },
                ),
                (
                    "step".to_string(),
                    FrontmatterFieldDecl {
                        field_type: "int".into(),
                        allowed: None,
                    },
                ),
            ]
            .into_iter()
            .collect(),
        );
        progress
    }

    fn resolve(node: &NodeDef, artifacts: &Path) -> Vec<LiveArtifactIO> {
        resolve_live(node, artifacts, 1, None, &HashSet::new())
    }

    #[test]
    fn a_declared_artifact_not_yet_published_is_listed_not_produced() {
        let tmp = tempfile::TempDir::new().unwrap();
        let node = node(
            vec![
                declared("progress", PortType::Markdown),
                declared("screens", PortType::ImageList),
            ],
            false,
        );
        let live = resolve(&node, tmp.path());
        assert_eq!(live.len(), 2);
        assert!(live.iter().all(|l| l.declared && l.files.is_empty()));
        assert!(live.iter().all(|l| l.updated_at.is_none()));
        assert_eq!(live[0].path, "designer/iter-1/live/progress.md");
        assert_eq!(live[1].path, "designer/iter-1/live/screens/");
    }

    #[test]
    fn declared_then_free_artifacts_with_their_frontmatter_and_schema_warnings() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("designer/iter-1/live");
        fs::create_dir_all(dir.join("draft-screens")).unwrap();
        fs::write(
            dir.join("progress.md"),
            "---\nstatus: drafting\nstep: three of five\n---\nWorking.\n",
        )
        .unwrap();
        fs::write(dir.join("notes-for-builder.md"), "Notes.\n").unwrap();
        fs::write(dir.join("draft-screens/cart.png"), "png").unwrap();
        fs::write(dir.join("preview.html"), "<p>hi</p>").unwrap();
        // Scratch files are not artifacts.
        fs::write(dir.join(".progress.md.swp"), "x").unwrap();
        fs::write(dir.join("progress.md.tmp"), "x").unwrap();

        let node = node(vec![progress_with_schema()], true);
        let live = resolve(&node, tmp.path());
        let names: Vec<(&str, bool, PortType)> = live
            .iter()
            .map(|l| (l.name.as_str(), l.declared, l.port_type))
            .collect();
        assert_eq!(
            names,
            vec![
                ("progress", true, PortType::Markdown),
                ("draft-screens", false, PortType::ImageList),
                ("notes-for-builder", false, PortType::Markdown),
                ("preview", false, PortType::Html),
            ]
        );

        let progress = &live[0];
        assert_eq!(
            progress.files[0].frontmatter.as_ref().unwrap()["status"],
            serde_json::json!("drafting")
        );
        assert_eq!(
            progress.schema_warnings.len(),
            1,
            "{:?}",
            progress.schema_warnings
        );
        assert_eq!(progress.schema_warnings[0].field, "step");
        assert!(progress.updated_at.is_some());

        let screens = &live[1];
        assert_eq!(screens.path, "designer/iter-1/live/draft-screens/");
        assert_eq!(screens.files.len(), 1);
        // A free artifact has no schema to miss.
        assert!(live[2].schema_warnings.is_empty());
    }

    #[test]
    fn a_free_folder_lists_every_file_it_holds() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("designer/iter-1/live/notes");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.md"), "a").unwrap();
        fs::write(dir.join("b.png"), "png").unwrap();
        let live = resolve(&node(vec![], true), tmp.path());
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].name, "notes");
        assert!(!live[0].declared);
        assert_eq!(live[0].files.len(), 2);
    }

    #[test]
    fn what_the_node_published_shows_even_with_free_publication_off() {
        // The switch only tells the agent it may; the Run view shows what is
        // in `live/` all the same — hiding a file on disk would mislead.
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("designer/iter-1/live");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("extra.md"), "x").unwrap();
        let live = resolve(
            &node(vec![declared("progress", PortType::Markdown)], false),
            tmp.path(),
        );
        let names: Vec<(&str, bool)> = live.iter().map(|l| (l.name.as_str(), l.declared)).collect();
        assert_eq!(names, vec![("progress", true), ("extra", false)]);
    }

    #[test]
    fn a_conforming_frontmatter_carries_no_warning() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("designer/iter-1/live");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("progress.md"),
            "---\nstatus: ready\nstep: 3\n---\n",
        )
        .unwrap();
        let live = resolve(&node(vec![progress_with_schema()], false), tmp.path());
        assert!(
            live[0].schema_warnings.is_empty(),
            "{:?}",
            live[0].schema_warnings
        );
    }

    #[test]
    fn each_iteration_reads_its_own_live_folder() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("designer/iter-1/live");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("progress.md"), "iteration 1").unwrap();
        let node = node(vec![declared("progress", PortType::Markdown)], true);
        let live = resolve_live(&node, tmp.path(), 2, None, &HashSet::new());
        assert_eq!(live.len(), 1);
        assert!(live[0].files.is_empty());
    }
}
