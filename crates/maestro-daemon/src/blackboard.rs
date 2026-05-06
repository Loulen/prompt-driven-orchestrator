use std::path::{Path, PathBuf};

#[allow(dead_code)]
pub fn artifact_path(artifacts_dir: &Path, node_id: &str, iter: i64, port_name: &str) -> PathBuf {
    artifacts_dir
        .join(node_id)
        .join(format!("iter-{iter}"))
        .join(format!("{port_name}.md"))
}

#[allow(dead_code)]
pub fn artifact_exists(artifacts_dir: &Path, node_id: &str, iter: i64, port_name: &str) -> bool {
    artifact_path(artifacts_dir, node_id, iter, port_name).exists()
}

#[allow(dead_code)]
pub fn input_path(artifacts_dir: &Path) -> PathBuf {
    artifacts_dir.join("_input.md")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::fs;

    #[test]
    fn single_port_single_iter_path() {
        let dir = Path::new("/repo/.maestro/artifacts");
        let path = artifact_path(dir, "planner", 1, "plan");
        assert_eq!(
            path,
            PathBuf::from("/repo/.maestro/artifacts/planner/iter-1/plan.md")
        );
    }

    #[test]
    fn multi_iter_path() {
        let dir = Path::new("/repo/.maestro/artifacts");
        let path = artifact_path(dir, "reviewer", 3, "review");
        assert_eq!(
            path,
            PathBuf::from("/repo/.maestro/artifacts/reviewer/iter-3/review.md")
        );
    }

    #[test]
    fn input_md_path() {
        let dir = Path::new("/repo/.maestro/artifacts");
        assert_eq!(
            input_path(dir),
            PathBuf::from("/repo/.maestro/artifacts/_input.md")
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
        let node_dir = artifacts_dir.join("planner").join("iter-1");
        fs::create_dir_all(&node_dir).unwrap();
        fs::write(node_dir.join("plan.md"), "# Plan").unwrap();
        assert!(artifact_exists(&artifacts_dir, "planner", 1, "plan"));
    }

    #[test]
    fn path_arithmetic_matches_canonical_schema() {
        let base = Path::new(
            "/home/user/repo/.maestro/runs/20260506-1200-abc1234/worktree/.maestro/artifacts",
        );
        let path = artifact_path(base, "implementer-1", 2, "summary");
        assert_eq!(
            path.to_str().unwrap(),
            "/home/user/repo/.maestro/runs/20260506-1200-abc1234/worktree/.maestro/artifacts/implementer-1/iter-2/summary.md"
        );
    }
}
