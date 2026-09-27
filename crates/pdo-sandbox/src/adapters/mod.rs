//! The sandbox adapters: the docker CLI, the Dockerfiles and the staged homes on
//! disk, the SQLite profile store, what is read from the host, and the HTTP routes.

pub mod docker_cli;
pub mod dockerfiles;
pub mod env;
pub mod host;
pub mod http;
pub mod sqlite_profiles;
pub mod staging;

use std::sync::Arc;

use crate::application::prepare::SandboxRuntime;

/// The runtime of a local daemon: the docker binary (the per-daemon override, else
/// `docker`), the staged homes and the Dockerfiles on the local filesystem.
pub fn local_runtime(docker_bin: Option<&str>) -> SandboxRuntime {
    SandboxRuntime {
        docker: Arc::new(docker_cli::ProcessDocker::from_override(docker_bin)),
        staging: Arc::new(staging::FsStaging),
        dockerfiles: Arc::new(dockerfiles::FsDockerfiles),
    }
}
