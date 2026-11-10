//! The sandbox section: the instance default sandbox, the staging profiles'
//! names, whether Docker can run a sandbox, and the host home a profile entry
//! is relative to — so the launch dialog learns all of it in ONE fetch.

use std::sync::Arc;

use pdo_sandbox::adapters::instance_defaults::{
    default_sandbox, docker_availability, profile_names,
};
use pdo_sandbox::application::docker_probe::DockerProbeCache;
use pdo_sandbox::application::ports::{DockerCli, SandboxProfileRepository};
use serde_json::{Map, Value};

use super::{json, section, HostHome};
use crate::application::ports::{BoxFuture, SettingsSection};
use crate::domain::instance_config::InstanceConfig;

pub struct SandboxSection {
    pub profiles: Arc<dyn SandboxProfileRepository>,
    pub docker: Arc<dyn DockerCli>,
    /// TTL cache (~10 s) of the Docker probe: lazily refreshed when stale — not
    /// boot-once (a user may start Docker mid-session) and not per fetch (a
    /// `PUT` of an unrelated knob would pay a docker round-trip).
    pub docker_probe: DockerProbeCache,
    pub home: HostHome,
}

impl SandboxSection {
    pub fn new(
        profiles: Arc<dyn SandboxProfileRepository>,
        docker: Arc<dyn DockerCli>,
        home: HostHome,
    ) -> Self {
        Self {
            profiles,
            docker,
            docker_probe: tokio::sync::Mutex::new(None),
            home,
        }
    }
}

impl SettingsSection for SandboxSection {
    fn disclose<'a>(
        &'a self,
        c: &'a InstanceConfig,
    ) -> BoxFuture<'a, Result<Map<String, Value>, String>> {
        Box::pin(async move {
            let default =
                default_sandbox(self.profiles.as_ref(), c.default_sandbox.as_deref()).await;
            let docker = docker_availability(&self.docker_probe, self.docker.clone()).await;
            let names = profile_names(self.profiles.as_ref())
                .await
                .map_err(|e| format!("encountered unexpected or invalid data: {e}"))?;
            // An observed FACT, not a tier: the filesystem explorer yields
            // ABSOLUTE paths while a profile entry is RELATIVE to `$HOME`.
            let home = self.home.path().map(|h| h.to_string_lossy().into_owned());
            Ok(section([
                ("default_sandbox", json(default)),
                ("sandbox_profiles", json(names)),
                ("home", json(home)),
                ("sandbox_docker", json(docker)),
            ]))
        })
    }
}
