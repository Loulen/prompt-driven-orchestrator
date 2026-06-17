use std::path::{Path, PathBuf};

pub fn port_dir(artifacts_dir: &Path, node_id: &str, iter: i64, port_name: &str) -> PathBuf {
    artifacts_dir
        .join(node_id)
        .join(format!("iter-{iter}"))
        .join(port_name)
}

pub fn artifact_path(artifacts_dir: &Path, node_id: &str, iter: i64, port_name: &str) -> PathBuf {
    port_dir(artifacts_dir, node_id, iter, port_name).join("output.md")
}

#[allow(dead_code)]
pub fn artifact_exists(artifacts_dir: &Path, node_id: &str, iter: i64, port_name: &str) -> bool {
    artifact_path(artifacts_dir, node_id, iter, port_name).exists()
}

pub fn input_path(artifacts_dir: &Path) -> PathBuf {
    artifacts_dir.join("_input").join("output.md")
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
        let base = Path::new(
            "/home/user/repo/.pdo/runs/20260506-1200-abc1234/worktree/.pdo/artifacts",
        );
        let path = artifact_path(base, "implementer-1", 2, "summary");
        assert_eq!(
            path.to_str().unwrap(),
            "/home/user/repo/.pdo/runs/20260506-1200-abc1234/worktree/.pdo/artifacts/implementer-1/iter-2/summary/output.md"
        );
    }
}
