//! Whether this host can launch a sandboxed Run right now (#410): the advisory
//! Docker probe `GET /settings` surfaces, cached briefly.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::image::probe_docker;
use super::ports::DockerCli;
use crate::domain::image::{DockerProbe, DOCKER_DAEMON_UNREACHABLE_MSG};

/// TTL of the Docker availability probe cache: long enough that a burst of
/// `/settings` fetches pays at most one docker round-trip, short enough that a user
/// who starts Docker mid-session sees `full`/`minimal` un-gray within seconds.
const DOCKER_PROBE_TTL: Duration = Duration::from_secs(10);
/// Hard cap on how long the probe may block the `/settings` response: a cold or
/// wedged Docker daemon must never hang the settings page.
const DOCKER_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// The last probe and when it ran. Lazily refreshed when stale — not boot-once (a user
/// may start Docker mid-session) and not per-fetch (a `PUT` of an unrelated knob would
/// pay a docker round-trip).
pub type DockerProbeCache = tokio::sync::Mutex<Option<(Instant, DockerProbe)>>;

/// Return the cached Docker availability probe, refreshing it under `spawn_blocking`
/// with a short `timeout` when stale. Never shells out on the runtime thread. The
/// result is advisory only — the run-advance fail-fast stays the authoritative gate.
pub async fn probe_cached(cache: &DockerProbeCache, docker: Arc<dyn DockerCli>) -> DockerProbe {
    probe_cached_with(cache, docker, DOCKER_PROBE_TTL, DOCKER_PROBE_TIMEOUT).await
}

async fn probe_cached_with(
    cache: &DockerProbeCache,
    docker: Arc<dyn DockerCli>,
    ttl: Duration,
    timeout: Duration,
) -> DockerProbe {
    let mut guard = cache.lock().await;
    if let Some((at, probe)) = guard.as_ref() {
        if at.elapsed() < ttl {
            return probe.clone();
        }
    }
    let probe = match tokio::time::timeout(
        timeout,
        tokio::task::spawn_blocking(move || probe_docker(docker.as_ref())),
    )
    .await
    {
        Ok(Ok(p)) => p,
        // Timed out or the blocking task panicked/joined-error: treat as unavailable
        // for this fetch (advisory), and don't cache the transient failure long — a
        // fresh probe runs next time the TTL check sees an entry we still stamp now.
        _ => DockerProbe {
            available: false,
            reason: Some(DOCKER_DAEMON_UNREACHABLE_MSG.to_string()),
        },
    };
    *guard = Some((Instant::now(), probe.clone()));
    probe
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::testing::ScriptedDocker;

    #[tokio::test]
    async fn a_fresh_probe_is_served_from_the_cache() {
        let cache = DockerProbeCache::default();
        let docker = Arc::new(ScriptedDocker::happy());
        assert!(probe_cached(&cache, docker.clone()).await.available);
        assert!(probe_cached(&cache, docker.clone()).await.available);
        assert_eq!(docker.calls().len(), 1, "one round-trip per TTL window");
    }

    #[tokio::test]
    async fn a_stale_probe_is_refreshed() {
        let cache = DockerProbeCache::default();
        let docker = Arc::new(ScriptedDocker::happy());
        let never = Duration::from_secs(1);
        probe_cached_with(&cache, docker.clone(), Duration::ZERO, never).await;
        probe_cached_with(&cache, docker.clone(), Duration::ZERO, never).await;
        assert_eq!(docker.calls().len(), 2);
    }

    #[tokio::test]
    async fn a_wedged_docker_answers_unreachable_within_the_timeout() {
        let cache = DockerProbeCache::default();
        let docker = Arc::new(ScriptedDocker::answering(|_| {
            std::thread::sleep(Duration::from_millis(300));
            ScriptedDocker::ok("")
        }));
        let probe = probe_cached_with(
            &cache,
            docker,
            Duration::from_secs(10),
            Duration::from_millis(20),
        )
        .await;
        assert!(!probe.available);
        assert_eq!(probe.reason.as_deref(), Some(DOCKER_DAEMON_UNREACHABLE_MSG));
    }
}
