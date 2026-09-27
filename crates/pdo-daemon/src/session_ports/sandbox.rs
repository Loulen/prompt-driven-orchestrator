//! The `docker exec` of a session into its Run's sandbox container (#407), as
//! the sandbox side builds it for the session context.

use std::path::{Path, PathBuf};

use pdo_session::domain::launch::{ContainerExec, SandboxWrap};

/// One session's way into its Run's container.
pub(crate) struct SandboxExec {
    /// The `docker` binary to invoke (per-daemon override → `"docker"`).
    docker_bin: String,
    run_id: String,
    /// The session marker: **MUST** equal the tmux session name the kill path
    /// uses (`PDO_SBX_SESSION`), or the targeted `/proc` kill misses its tree.
    marker: String,
    /// The session's working dir → `docker exec -w <workdir>` (the load-bearing
    /// cwd inside the container).
    workdir: PathBuf,
    /// The resolved harness's staging-set env (#708), forwarded on the exec.
    set_env: Vec<(String, String)>,
}

impl SandboxExec {
    pub(crate) fn new(
        docker_override: Option<&str>,
        run_id: &str,
        marker: &str,
        workdir: &Path,
        set_env: &[(String, String)],
    ) -> Self {
        Self {
            docker_bin: docker_override.unwrap_or("docker").to_string(),
            run_id: run_id.to_string(),
            marker: marker.to_string(),
            workdir: workdir.to_path_buf(),
            set_env: set_env.to_vec(),
        }
    }

    /// The wrap the session launch splices its tail into.
    pub(crate) fn wrap(&self) -> SandboxWrap<'_> {
        SandboxWrap {
            exec: self,
            set_env: &self.set_env,
        }
    }
}

impl ContainerExec for SandboxExec {
    fn exec_argv(&self, env: &[(String, String)]) -> Vec<String> {
        let mut argv = vec![self.docker_bin.clone()];
        argv.extend(pdo_sandbox::domain::container::exec_prefix_with_env(
            &self.run_id,
            pdo_sandbox::adapters::host::host_uid(),
            pdo_sandbox::adapters::host::host_gid(),
            &self.workdir,
            &self.marker,
            env,
        ));
        argv
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_exec_is_the_sandbox_prefix_behind_the_docker_binary() {
        let exec = SandboxExec::new(None, "r1", "pdo-r1-solo-iter-1", Path::new("/wd"), &[]);
        let env = [("PDO_OUTPUT_out".to_string(), "/a".to_string())];
        let mut expected = vec!["docker".to_string()];
        expected.extend(pdo_sandbox::domain::container::exec_prefix_with_env(
            "r1",
            pdo_sandbox::adapters::host::host_uid(),
            pdo_sandbox::adapters::host::host_gid(),
            Path::new("/wd"),
            "pdo-r1-solo-iter-1",
            &env,
        ));
        assert_eq!(exec.exec_argv(&env), expected);
        let custom = SandboxExec::new(Some("/fake/docker"), "r1", "m", Path::new("/wd"), &[]);
        assert_eq!(custom.exec_argv(&[])[0], "/fake/docker");
    }

    #[test]
    fn the_host_side_daemon_url_is_the_one_the_sandbox_resolver_gives() {
        assert_eq!(
            pdo_session::domain::launch::host_daemon_url(6193),
            pdo_sandbox::domain::container::daemon_url(6193, false)
        );
    }
}
