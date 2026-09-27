//! The docker CLI, run as a child process: the one implementation of
//! [`DockerCli`].

use std::process::Command;

use crate::application::ports::{DockerCli, DockerOutput};

/// `docker` (or the per-daemon override, a test seam) invoked with
/// `std::process::Command`, synchronously.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessDocker {
    bin: String,
}

impl ProcessDocker {
    /// The docker binary at `bin` (a name resolved on `PATH`, or a path).
    pub fn new(bin: impl Into<String>) -> Self {
        Self { bin: bin.into() }
    }

    /// The per-daemon override when there is one, else `docker`.
    pub fn from_override(bin: Option<&str>) -> Self {
        Self::new(bin.unwrap_or("docker"))
    }

    /// The binary this adapter runs — what a session's `docker exec` tail names.
    pub fn binary(&self) -> &str {
        &self.bin
    }
}

impl DockerCli for ProcessDocker {
    fn run(&self, args: &[String]) -> std::io::Result<DockerOutput> {
        let output = Command::new(&self.bin).args(args).output()?;
        Ok(DockerOutput {
            success: output.status.success(),
            status: output.status.to_string(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

/// Stage 3 (ADR-0080): the adapter against a real docker daemon. Docker is not a
/// prerequisite of the suite: when no daemon answers, each case says so and passes
/// without asserting anything.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::container;
    use crate::application::image::probe_docker;

    /// The real docker, or the reason there is none.
    fn real_docker() -> Result<ProcessDocker, String> {
        let docker = ProcessDocker::new("docker");
        let probe = probe_docker(&docker);
        if probe.available {
            Ok(docker)
        } else {
            Err(probe.reason.unwrap_or_default())
        }
    }

    macro_rules! docker_or_skip {
        () => {
            match real_docker() {
                Ok(docker) => docker,
                Err(reason) => {
                    eprintln!("skipped: no usable docker daemon on this host ({reason})");
                    return;
                }
            }
        };
    }

    #[test]
    fn a_real_docker_reports_its_server_version() {
        let docker = docker_or_skip!();
        let out = docker
            .run(&[
                "version".into(),
                "--format".into(),
                "{{.Server.Version}}".into(),
            ])
            .unwrap();
        assert!(out.success, "{out:?}");
        assert!(!out.stdout.trim().is_empty(), "{out:?}");
        assert_eq!(out.status, "exit status: 0");
    }

    #[test]
    fn a_real_docker_says_an_unknown_image_is_absent() {
        let docker = docker_or_skip!();
        let out = docker
            .run(&[
                "image".into(),
                "inspect".into(),
                "pdo-sandbox-stage3-never-built:h-000000000000".into(),
            ])
            .unwrap();
        assert!(!out.success);
        assert_ne!(out.status, "exit status: 0");
    }

    #[test]
    fn a_real_docker_confirms_the_absent_container_sentinels() {
        let docker = docker_or_skip!();
        let run_id = format!("stage3-absent-{}", std::process::id());
        // `rm -f` of a container that never existed is the idempotent success the
        // cleanup relies on, whatever the docker version says on stderr.
        container::remove(&docker, &run_id).unwrap();
        // A binary probe in it cannot answer: an error, never a "binary absent".
        let err = container::binary_present_in_container(&docker, &run_id, 0, 0, "sh")
            .expect_err("an absent container cannot answer a binary probe");
        assert!(format!("{err:#}").contains("cannot answer"), "{err:#}");
    }

    #[test]
    fn a_missing_binary_is_a_spawn_error_not_an_exit_status() {
        let err = ProcessDocker::new("/nonexistent/pdo-no-docker")
            .run(&["version".into()])
            .unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }
}
