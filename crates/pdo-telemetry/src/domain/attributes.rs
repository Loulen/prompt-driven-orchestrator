//! What a measure is attached to (ADR-0090 §3): the instance, the Projet, the
//! repository, git, the pipeline, the harness and the model. The Run and node
//! ids, of unbounded cardinality, go on the traces only. The Actor (who
//! launched a Run, who answered it) is an attribute only when the setting
//! « include the Actor » is on (ADR-0090 §4): every method that builds the
//! attributes is told whether it may name it.

use pdo_kernel::Actor;
use serde::Serialize;
use ts_rs::TS;

/// The attribute keys, one place, so the page's « What is sent » and the export
/// cannot disagree.
pub mod keys {
    pub const INSTANCE: &str = "service.instance.id";
    pub const PROJECT: &str = "pdo.project";
    pub const REPOSITORY: &str = "pdo.repository";
    pub const BRANCH: &str = "pdo.branch";
    pub const COMMIT: &str = "pdo.commit";
    pub const PIPELINE: &str = "pdo.pipeline";
    pub const HARNESS: &str = "pdo.harness";
    pub const MODEL: &str = "pdo.model";
    pub const OUTCOME: &str = "pdo.outcome";
    pub const TOKEN_TYPE: &str = "pdo.token.type";
    pub const CONNECTION: &str = "pdo.connection";
    pub const RUN_ID: &str = "pdo.run.id";
    pub const NODE_ID: &str = "pdo.node.id";
    pub const ITERATION: &str = "pdo.node.iteration";
    /// Only with « include the Actor ».
    pub const ACTOR: &str = "pdo.actor";
}

/// Events of a node's span.
pub mod events {
    /// A person answered the node's declared wait.
    pub const WAIT_ANSWERED: &str = "pdo.wait.answered";
}

/// How an Actor is named on a measure: the identifier an extension gives a
/// person, the local operator, a Trigger. An agent is named by its kind only:
/// its Run and node ids would make the metrics unbounded, and it is no person.
pub fn actor_label(actor: &Actor) -> String {
    match actor {
        Actor::Identified { namespace, id } => format!("{namespace}:{id}"),
        Actor::Trigger { trigger_id } => format!("trigger:{trigger_id}"),
        other => other.kind().to_string(),
    }
}

/// The Actor a stored payload names under `key`, labelled; `None` when it
/// names none or one this version cannot read.
pub fn actor_in(payload: Option<&serde_json::Value>, key: &str) -> Option<String> {
    let actor = payload?.get(key)?.clone();
    serde_json::from_value::<Actor>(actor)
        .ok()
        .map(|actor| actor_label(&actor))
}

/// What a Run's measures are attached to, resolved once per Run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunFacts {
    /// The Projet's name.
    pub project: Option<String>,
    /// The repository's label (`repository_label`), never a path.
    pub repository: Option<String>,
    pub branch: Option<String>,
    /// The commit the Run was cut from.
    pub commit: Option<String>,
    pub pipeline: Option<String>,
    /// The Run's harness, when it chose one.
    pub harness: Option<String>,
    /// Who launched the Run ([`actor_label`]), sent only when `include_actor`.
    pub actor: Option<String>,
}

impl RunFacts {
    /// The attributes of a Run's metrics: the facts that are known, the Actor
    /// only when `include_actor`.
    pub fn attributes(&self, include_actor: bool) -> Vec<(&'static str, String)> {
        let actor = self.actor.clone().filter(|_| include_actor);
        [
            (keys::PROJECT, &self.project),
            (keys::REPOSITORY, &self.repository),
            (keys::BRANCH, &self.branch),
            (keys::COMMIT, &self.commit),
            (keys::PIPELINE, &self.pipeline),
            (keys::HARNESS, &self.harness),
            (keys::ACTOR, &actor),
        ]
        .into_iter()
        .filter_map(|(key, value)| Some((key, value.clone()?)))
        .collect()
    }

    /// The attributes of a node's metrics: the Run's, with the node's own
    /// harness and model, which win over the Run's.
    pub fn node_attributes(
        &self,
        harness: Option<&str>,
        model: Option<&str>,
        include_actor: bool,
    ) -> Vec<(&'static str, String)> {
        let mut attributes = self.attributes(include_actor);
        if let Some(harness) = harness.filter(|h| !h.is_empty()) {
            attributes.retain(|(key, _)| *key != keys::HARNESS);
            attributes.push((keys::HARNESS, harness.to_string()));
        }
        if let Some(model) = model.filter(|m| !m.is_empty()) {
            attributes.push((keys::MODEL, model.to_string()));
        }
        attributes
    }
}

/// The metrics the instance exports, by name.
pub mod metrics {
    pub const RUNS_STARTED: &str = "pdo.runs.started";
    pub const RUNS_FINISHED: &str = "pdo.runs.finished";
    pub const RUNS_FAILED: &str = "pdo.runs.failed";
    pub const NODE_DURATION: &str = "pdo.node.duration";
    pub const NODE_ACTIVE_DURATION: &str = "pdo.node.active_duration";
    pub const NODE_WAIT_DURATION: &str = "pdo.node.wait_duration";
    pub const WAITS_DECLARED: &str = "pdo.waits.declared";
    pub const COST: &str = "pdo.cost";
    pub const TOKENS: &str = "pdo.tokens";
    pub const SESSIONS_LIVE: &str = "pdo.sessions.live";
    pub const WEBSOCKETS_OPEN: &str = "pdo.websockets.open";
    pub const HTTP_DURATION: &str = "http.server.request.duration";
}

/// « What is sent », as the page shows it: read from the same constants the
/// export uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct TelemetryCatalogue {
    /// On every metric and every span.
    pub resource: Vec<String>,
    /// On the metrics of a Run and of a node.
    pub run_attributes: Vec<String>,
    /// On the traces only.
    pub trace_only: Vec<String>,
    /// On the Run and node metrics and on the traces, only with « include the
    /// Actor ».
    pub with_actor: Vec<String>,
    pub metrics: Vec<String>,
    pub traces: Vec<String>,
}

pub fn catalogue() -> TelemetryCatalogue {
    let owned = |items: &[&str]| items.iter().map(|s| s.to_string()).collect();
    TelemetryCatalogue {
        resource: owned(&["service.name", "service.version", keys::INSTANCE]),
        run_attributes: owned(&[
            keys::PROJECT,
            keys::REPOSITORY,
            keys::BRANCH,
            keys::COMMIT,
            keys::PIPELINE,
            keys::HARNESS,
            keys::MODEL,
        ]),
        trace_only: owned(&[keys::RUN_ID, keys::NODE_ID]),
        with_actor: owned(&[keys::ACTOR]),
        metrics: owned(&[
            metrics::RUNS_STARTED,
            metrics::RUNS_FINISHED,
            metrics::RUNS_FAILED,
            metrics::NODE_DURATION,
            metrics::NODE_ACTIVE_DURATION,
            metrics::NODE_WAIT_DURATION,
            metrics::WAITS_DECLARED,
            metrics::COST,
            metrics::TOKENS,
            metrics::SESSIONS_LIVE,
            metrics::WEBSOCKETS_OPEN,
            metrics::HTTP_DURATION,
        ]),
        traces: owned(&["HTTP requests", "Run → node"]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn facts() -> RunFacts {
        RunFacts {
            project: Some("Core".into()),
            repository: Some("https://git.example.org/org/app.git".into()),
            branch: Some("main".into()),
            commit: Some("abc123".into()),
            pipeline: Some("implement".into()),
            harness: None,
            actor: Some("test:alice".into()),
        }
    }

    #[test]
    fn a_run_carries_the_known_facts_only() {
        let keys: Vec<&str> = facts().attributes(false).iter().map(|(k, _)| *k).collect();
        assert_eq!(
            keys,
            vec![
                keys::PROJECT,
                keys::REPOSITORY,
                keys::BRANCH,
                keys::COMMIT,
                keys::PIPELINE
            ]
        );
    }

    #[test]
    fn a_node_adds_its_harness_and_model() {
        let mut run = facts();
        run.harness = Some("run-harness".into());
        let attributes = run.node_attributes(Some("node-harness"), Some("model-x"), false);
        assert!(attributes.contains(&(keys::HARNESS, "node-harness".into())));
        assert!(!attributes.contains(&(keys::HARNESS, "run-harness".into())));
        assert!(attributes.contains(&(keys::MODEL, "model-x".into())));
    }

    #[test]
    fn no_metric_attribute_identifies_a_run_or_a_node() {
        let attributes = facts().node_attributes(Some("h"), Some("m"), true);
        assert!(attributes
            .iter()
            .all(|(k, _)| *k != keys::RUN_ID && *k != keys::NODE_ID));
        let sent = catalogue();
        assert!(!sent.run_attributes.contains(&keys::RUN_ID.to_string()));
    }

    #[test]
    fn the_actor_is_named_only_when_it_may_be() {
        let has_actor = |attributes: Vec<(&'static str, String)>| {
            attributes.iter().any(|(k, _)| *k == keys::ACTOR)
        };
        assert!(!has_actor(facts().attributes(false)));
        assert!(!has_actor(facts().node_attributes(Some("h"), None, false)));
        assert!(facts()
            .attributes(true)
            .contains(&(keys::ACTOR, "test:alice".into())));
        assert!(has_actor(facts().node_attributes(Some("h"), None, true)));
        let sent = catalogue();
        assert!(!sent.run_attributes.contains(&keys::ACTOR.to_string()));
        assert_eq!(sent.with_actor, vec![keys::ACTOR.to_string()]);
    }

    #[test]
    fn an_actor_is_labelled_by_who_it_is() {
        assert_eq!(actor_label(&Actor::LocalOperator), "local_operator");
        assert_eq!(
            actor_label(&Actor::Identified {
                namespace: "test".into(),
                id: "alice".into()
            }),
            "test:alice"
        );
        assert_eq!(
            actor_label(&Actor::Trigger {
                trigger_id: "nightly".into()
            }),
            "trigger:nightly"
        );
        assert_eq!(
            actor_label(&Actor::Agent {
                run_id: "r1".into(),
                node_id: "n1".into()
            }),
            "agent",
            "an agent's ids never reach the metrics"
        );
        let payload = serde_json::json!({
            "created_by": { "kind": "identified", "namespace": "test", "id": "bob" },
            "other": { "kind": "from-a-later-version" },
        });
        assert_eq!(
            actor_in(Some(&payload), "created_by").as_deref(),
            Some("test:bob")
        );
        assert_eq!(actor_in(Some(&payload), "other"), None);
        assert_eq!(actor_in(None, "created_by"), None);
    }
}
