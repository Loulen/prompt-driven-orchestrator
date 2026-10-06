use std::path::{Path, PathBuf};

pub(crate) fn port_dir(artifacts_dir: &Path, node_id: &str, iter: i64, port_name: &str) -> PathBuf {
    artifacts_dir
        .join(node_id)
        .join(format!("iter-{iter}"))
        .join(port_name)
}

pub(crate) fn artifact_path(
    artifacts_dir: &Path,
    node_id: &str,
    iter: i64,
    port_name: &str,
) -> PathBuf {
    port_dir(artifacts_dir, node_id, iter, port_name).join("output.md")
}

/// Path of an `html` output port's file. A dedicated helper localizes the
/// `output.html` choice to the output sites that emit it, keeping the
/// type-blind input side (which always reads `output.md`) untouched.
pub(crate) fn artifact_path_html(
    artifacts_dir: &Path,
    node_id: &str,
    iter: i64,
    port_name: &str,
) -> PathBuf {
    port_dir(artifacts_dir, node_id, iter, port_name).join("output.html")
}

#[allow(dead_code)]
pub(crate) fn artifact_exists(
    artifacts_dir: &Path,
    node_id: &str,
    iter: i64,
    port_name: &str,
) -> bool {
    artifact_path(artifacts_dir, node_id, iter, port_name).exists()
}

pub(crate) fn input_path(artifacts_dir: &Path) -> PathBuf {
    artifacts_dir.join("_input").join("output.md")
}

/// The Blackboard directory of the files a user imported into a running node
/// (#971, « Fichier importé en cours de Run »), a sibling of `_input/`: inside
/// `.pdo/artifacts/`, so never committed and removed with the Run's worktree.
const ATTACHMENTS_DIR: &str = "_attachments";

/// `<artifacts>/_attachments/<node-id>/` — one folder per node, so who received
/// what stays readable.
pub(crate) fn node_attachments_dir(artifacts_dir: &Path, node_id: &str) -> PathBuf {
    artifacts_dir.join(ATTACHMENTS_DIR).join(node_id)
}

/// The name an imported file may take: the last path component of what the
/// browser sent (no traversal), trimmed; empty, `.` and `..` are refused.
pub(crate) fn sanitize_import_name(raw: &str) -> Result<String, String> {
    let last = raw.rsplit(['/', '\\']).next().unwrap_or("").trim();
    if last.is_empty() || last == "." || last == ".." || last.chars().any(char::is_control) {
        return Err(format!("invalid file name: {raw:?}"));
    }
    Ok(last.to_string())
}

/// The `n`-th suffixed variant of `name`: `contrat.pdf` → `contrat-1.pdf`,
/// `README` → `README-1`, `.env` → `.env-1`.
fn suffixed_name(name: &str, n: u32) -> String {
    match name.rfind('.') {
        Some(dot) if dot > 0 => format!("{}-{n}{}", &name[..dot], &name[dot..]),
        _ => format!("{name}-{n}"),
    }
}

/// Write each `(name, bytes)` into `dir` (created if missing) and return the
/// names they landed under, in order. A name already present — on disk or
/// earlier in the same import — is **never overwritten**: the file takes the
/// first free `-1`, `-2`… suffix. All or nothing: on a write error the files
/// this call already wrote are removed before the error is returned.
pub(crate) fn import_files(
    dir: &Path,
    files: &[(String, Vec<u8>)],
) -> std::io::Result<Vec<String>> {
    use std::io::Write;
    std::fs::create_dir_all(dir)?;
    let mut written: Vec<String> = Vec::new();
    let result = (|| {
        for (name, data) in files {
            let mut n = 0;
            loop {
                let candidate = if n == 0 {
                    name.clone()
                } else {
                    suffixed_name(name, n)
                };
                // `create_new` makes the "free name" check and the claim one
                // atomic step: a concurrent import can never overwrite this one.
                match std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(dir.join(&candidate))
                {
                    Ok(mut file) => {
                        written.push(candidate);
                        file.write_all(data)?;
                        break;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(())
    })();
    if let Err(e) = result {
        for name in &written {
            let _ = std::fs::remove_file(dir.join(name));
        }
        return Err(e);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::fs;

    #[test]
    fn single_port_single_iter_path() {
        let dir = Path::new("/repo/.pdo/artifacts");
        let path = artifact_path(dir, "planner", 1, "plan");
        assert_eq!(
            path,
            PathBuf::from("/repo/.pdo/artifacts/planner/iter-1/plan/output.md")
        );
    }

    #[test]
    fn multi_iter_path() {
        let dir = Path::new("/repo/.pdo/artifacts");
        let path = artifact_path(dir, "reviewer", 3, "review");
        assert_eq!(
            path,
            PathBuf::from("/repo/.pdo/artifacts/reviewer/iter-3/review/output.md")
        );
    }

    #[test]
    fn html_artifact_path_uses_output_html() {
        let dir = Path::new("/repo/.pdo/artifacts");
        let path = artifact_path_html(dir, "designer", 1, "report");
        assert_eq!(
            path,
            PathBuf::from("/repo/.pdo/artifacts/designer/iter-1/report/output.html")
        );
    }

    #[test]
    fn html_artifact_path_honors_iteration() {
        let dir = Path::new("/repo/.pdo/artifacts");
        let path = artifact_path_html(dir, "designer", 4, "report");
        assert_eq!(
            path,
            PathBuf::from("/repo/.pdo/artifacts/designer/iter-4/report/output.html")
        );
    }

    #[test]
    fn html_and_markdown_paths_share_a_port_dir_but_differ_in_filename() {
        let dir = Path::new("/repo/.pdo/artifacts");
        let md = artifact_path(dir, "designer", 1, "report");
        let html = artifact_path_html(dir, "designer", 1, "report");
        assert_eq!(md.parent(), html.parent());
        assert_ne!(md, html);
        assert_eq!(html.file_name().unwrap(), "output.html");
    }

    #[test]
    fn port_dir_returns_directory_for_port() {
        let dir = Path::new("/repo/.pdo/artifacts");
        let pd = port_dir(dir, "reviewer", 3, "review");
        assert_eq!(
            pd,
            PathBuf::from("/repo/.pdo/artifacts/reviewer/iter-3/review")
        );
    }

    #[test]
    fn input_path_points_to_directory_based_output_md() {
        let dir = Path::new("/repo/.pdo/artifacts");
        assert_eq!(
            input_path(dir),
            PathBuf::from("/repo/.pdo/artifacts/_input/output.md")
        );
    }

    #[test]
    fn artifact_exists_returns_false_for_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        fs::create_dir_all(&artifacts_dir).unwrap();
        assert!(!artifact_exists(&artifacts_dir, "planner", 1, "plan"));
    }

    #[test]
    fn artifact_exists_returns_true_when_present() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        let port_d = artifacts_dir.join("planner").join("iter-1").join("plan");
        fs::create_dir_all(&port_d).unwrap();
        fs::write(port_d.join("output.md"), "# Plan").unwrap();
        assert!(artifact_exists(&artifacts_dir, "planner", 1, "plan"));
    }

    #[test]
    fn path_arithmetic_matches_canonical_schema() {
        let base =
            Path::new("/home/user/repo/.pdo/runs/20260506-1200-abc1234/worktree/.pdo/artifacts");
        let path = artifact_path(base, "implementer-1", 2, "summary");
        assert_eq!(
            path.to_str().unwrap(),
            "/home/user/repo/.pdo/runs/20260506-1200-abc1234/worktree/.pdo/artifacts/implementer-1/iter-2/summary/output.md"
        );
    }

    #[test]
    fn node_attachments_dir_is_a_sibling_of_input() {
        let dir = Path::new("/repo/.pdo/artifacts");
        assert_eq!(
            node_attachments_dir(dir, "grill"),
            PathBuf::from("/repo/.pdo/artifacts/_attachments/grill")
        );
    }

    #[test]
    fn sanitize_import_name_keeps_the_last_component_only() {
        assert_eq!(sanitize_import_name("contrat.pdf").unwrap(), "contrat.pdf");
        assert_eq!(sanitize_import_name("../../etc/passwd").unwrap(), "passwd");
        assert_eq!(sanitize_import_name("C:\\tmp\\a.txt").unwrap(), "a.txt");
        assert!(sanitize_import_name("").is_err());
        assert!(sanitize_import_name("..").is_err());
        assert!(sanitize_import_name("dir/").is_err());
        assert!(sanitize_import_name("a\nb").is_err());
    }

    #[test]
    fn suffixed_name_goes_before_the_extension() {
        assert_eq!(suffixed_name("contrat.pdf", 1), "contrat-1.pdf");
        assert_eq!(suffixed_name("README", 2), "README-2");
        assert_eq!(suffixed_name(".env", 1), ".env-1");
    }

    #[test]
    fn import_files_never_overwrites_and_suffixes() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("_attachments").join("n");
        let first = import_files(&dir, &[("contrat.pdf".into(), b"v1".to_vec())]).unwrap();
        assert_eq!(first, vec!["contrat.pdf"]);
        let second = import_files(
            &dir,
            &[
                ("contrat.pdf".into(), b"v2".to_vec()),
                ("contrat.pdf".into(), b"v3".to_vec()),
            ],
        )
        .unwrap();
        assert_eq!(second, vec!["contrat-1.pdf", "contrat-2.pdf"]);
        assert_eq!(fs::read(dir.join("contrat.pdf")).unwrap(), b"v1");
        assert_eq!(fs::read(dir.join("contrat-1.pdf")).unwrap(), b"v2");
        assert_eq!(fs::read(dir.join("contrat-2.pdf")).unwrap(), b"v3");
    }
}
