//! The reads of the shared configuration go through the Autorisation
//! d'opération: the pipelines (their YAML, their versions, their document,
//! their shared draft), the library, the agentic profiles, the skills and
//! their folders, the instance settings and the audit journal. Each is the
//! Operation `<resource>` × `read`, in no Projet; a refusal answers `403` with
//! the refusal's body, so a view shows its sentence.
//!
//! A layer of the whole router, like the guard of a Run's reads: these routes
//! belong to several contexts, and a read added tomorrow under one of their
//! paths is guarded without anyone remembering to. It runs inside `identify`,
//! so the Actor is known. Without extension, nothing is asked.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::Method;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::Router;
use pdo_kernel::authorization::authorize_read;
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

/// `router`, each read of the shared configuration guarded by `host`.
pub(crate) fn guard_shared_reads(router: Router, host: Arc<dyn ExtensionHost>) -> Router {
    router.layer(middleware::from_fn_with_state(host, guard))
}

async fn guard(
    State(host): State<Arc<dyn ExtensionHost>>,
    request: Request,
    next: Next,
) -> Response {
    if host.allows_everything() {
        return next.run(request).await;
    }
    if let Some(resource) = shared_read(request.method(), request.uri().path()) {
        let actor = request
            .extensions()
            .get::<Actor>()
            .cloned()
            .unwrap_or(Actor::LocalOperator);
        let read = Operation::new(resource, Verb::Read);
        if let Err(denied) = authorize_read(host.as_ref(), &actor, &read) {
            return denied.into_response();
        }
    }
    next.run(request).await
}

/// The shared configuration a request reads: the Ressource of a `GET` or
/// `HEAD` under one of its paths, `None` for any other request.
fn shared_read(method: &Method, path: &str) -> Option<ResourceKind> {
    if method != Method::GET && method != Method::HEAD {
        return None;
    }
    let under = |prefix: &str| {
        path.strip_prefix(prefix)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    };
    if under("/pipelines") {
        Some(ResourceKind::Pipeline)
    } else if under("/library") {
        Some(ResourceKind::LibraryEntry)
    } else if under("/settings/agent-profiles") {
        Some(ResourceKind::AgentProfile)
    } else if under("/settings/skills") {
        Some(ResourceKind::Skill)
    } else if under("/settings/skill-folders") {
        Some(ResourceKind::SkillFolder)
    } else if path == "/settings" || under("/settings/provisioning") {
        Some(ResourceKind::InstanceSettings)
    } else if under("/audit") {
        Some(ResourceKind::Audit)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_read_of_the_shared_configuration_names_its_resource() {
        let get = Method::GET;
        for (path, resource) in [
            ("/pipelines", ResourceKind::Pipeline),
            ("/pipelines/review-loop", ResourceKind::Pipeline),
            ("/pipelines/review-loop/document", ResourceKind::Pipeline),
            ("/pipelines/review-loop/versions/3", ResourceKind::Pipeline),
            ("/pipelines/review-loop/draft", ResourceKind::Pipeline),
            ("/library", ResourceKind::LibraryEntry),
            ("/settings/agent-profiles", ResourceKind::AgentProfile),
            ("/settings/agent-profiles/p1", ResourceKind::AgentProfile),
            ("/settings/skills", ResourceKind::Skill),
            ("/settings/skills/s1/files/SKILL.md", ResourceKind::Skill),
            ("/settings/skill-folders", ResourceKind::SkillFolder),
            ("/settings/skill-folders/f1", ResourceKind::SkillFolder),
            ("/settings", ResourceKind::InstanceSettings),
            ("/settings/provisioning", ResourceKind::InstanceSettings),
            ("/audit", ResourceKind::Audit),
        ] {
            assert_eq!(shared_read(&get, path), Some(resource), "{path}");
        }
        assert_eq!(
            shared_read(&Method::HEAD, "/library"),
            Some(ResourceKind::LibraryEntry)
        );
    }

    #[test]
    fn a_mutation_or_another_read_is_not_guarded_here() {
        assert_eq!(shared_read(&Method::POST, "/pipelines"), None);
        assert_eq!(shared_read(&Method::PUT, "/settings"), None);
        assert_eq!(shared_read(&Method::DELETE, "/library/x"), None);
        for path in [
            "/runs",
            "/runs/r1/pipeline",
            "/pipelinesx",
            "/settings/sandbox-profiles",
            "/settings/skillsx",
            "/projects",
            "/stats/overview",
            "/auditx",
        ] {
            assert_eq!(shared_read(&Method::GET, path), None, "{path}");
        }
    }
}
