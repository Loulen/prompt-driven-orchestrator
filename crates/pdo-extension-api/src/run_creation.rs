//! The creation of a Run, as the extensions take part in it (API 0.9): the
//! data a creation carries for an extension, and the Run an extension creates
//! through its [`Host`](crate::Host).

use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The data a creation carries for the extensions: extension id → a JSON
/// value the core never interprets (`POST /runs` `{"extensions": {"<id>":
/// …}}`). The extension `<id>` checks its value
/// ([`Extension::validate_create`](crate::Extension::validate_create)) and
/// receives it when it stamps the new entity
/// ([`Extension::on_create_with_data`](crate::Extension::on_create_with_data)).
pub type ExtensionData = BTreeMap<String, Value>;

/// One repository of a [`NewRun`]: a path (or URL, as `POST /runs` takes it),
/// the branch it starts from (its local `HEAD` when `None`), and whether a
/// secondary is read-only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct RunRepo {
    pub repo: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub read_only: bool,
}

impl RunRepo {
    /// The repository `repo`, from its local `HEAD`, writable.
    pub fn new(repo: impl Into<String>) -> Self {
        Self {
            repo: repo.into(),
            base_branch: None,
            read_only: false,
        }
    }

    /// The same repository, starting from `branch`.
    pub fn from_branch(mut self, branch: impl Into<String>) -> Self {
        self.base_branch = Some(branch.into());
        self
    }

    /// The same repository, read-only (a secondary only: the primary is
    /// always writable).
    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }
}

/// Where a Run an extension creates comes from, as a person reads it: a short
/// `label` ("Nightly sync") and a `link`, a deep link of the interface
/// (`#/ext/<id>/<location>/<path>`, `#/runs/<id>`) toward what started it.
/// The core freezes it on the Run with the id of the extension that created
/// it, keeps it on a retry, and shows it as an "External source": a chip on
/// the Run's row (a click follows the link), a value of the list's filter, a
/// row of its Info tab. It is not a Trigger's `triggered_by`: the Triggers tab
/// never sees it.
///
/// Only an extension sets it ([`NewRun::from_source`]): `POST /runs` takes no
/// Provenance. The core refuses (`400`) a blank label, a label longer than
/// [`Provenance::MAX_LABEL`] characters, and a link that is not a deep link of
/// the interface (`#/ext/…` or `#/runs/…`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Provenance {
    pub label: String,
    pub link: String,
}

/// The deep links of the interface a Provenance may point at: an
/// extension's page, a Run.
const DEEP_LINK_PREFIXES: [&str; 2] = ["#/ext/", "#/runs/"];

impl Provenance {
    /// The longest label the core accepts, in characters.
    pub const MAX_LABEL: usize = 80;

    /// The Provenance `label`, linking to `link`.
    pub fn new(label: impl Into<String>, link: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            link: link.into(),
        }
    }

    /// Why the core refuses this Provenance, if it does: a blank or too long
    /// label, a link that is not a deep link of the interface.
    pub fn refusal(&self) -> Option<String> {
        let label = self.label.trim();
        if label.is_empty() {
            return Some("the label of a Provenance cannot be blank".into());
        }
        if label.chars().count() > Self::MAX_LABEL {
            return Some(format!(
                "the label of a Provenance holds at most {} characters",
                Self::MAX_LABEL
            ));
        }
        if label.chars().any(char::is_control) {
            return Some("the label of a Provenance cannot hold a control character".into());
        }
        let link = &self.link;
        let deep = DEEP_LINK_PREFIXES
            .iter()
            .any(|prefix| link.len() > prefix.len() && link.starts_with(prefix));
        if !deep || link.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Some(format!(
                "the link of a Provenance must be a deep link of the interface (`#/ext/…` or `#/runs/…`): `{link}`"
            ));
        }
        None
    }
}

/// The Run an extension asks its [`Host`](crate::Host) to create
/// ([`Host::create_run`](crate::Host::create_run)): what `POST /runs` takes,
/// for the fields an extension needs. The first repository is the primary,
/// the others are secondaries.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct NewRun {
    /// The pipeline to run: an id of the library, or a name the Run's
    /// repository holds.
    pub pipeline: String,
    /// The prompt of its entry node.
    pub input: String,
    /// Its repositories, the primary first. None: the Run has no repository,
    /// which the core refuses.
    pub repos: Vec<RunRepo>,
    /// Its display name; the core names it otherwise.
    pub name: Option<String>,
    /// The Projet it is launched in (ADR-0087), which must hold its primary
    /// repository; `None`: in no Projet.
    pub project_id: Option<String>,
    /// The data it carries for the extensions, as `POST /runs` `extensions`.
    pub extensions: ExtensionData,
    /// Where it comes from ([`Provenance`]); `None`: a Run like any other.
    /// Not part of [`NewRun::to_request_body`]: `POST /runs` takes none.
    pub provenance: Option<Provenance>,
}

impl NewRun {
    /// The Run of `pipeline` with the prompt `input`, in no repository yet.
    pub fn new(pipeline: impl Into<String>, input: impl Into<String>) -> Self {
        Self {
            pipeline: pipeline.into(),
            input: input.into(),
            ..Self::default()
        }
    }

    /// The same Run, with one more repository (the first one is the primary).
    pub fn in_repo(mut self, repo: RunRepo) -> Self {
        self.repos.push(repo);
        self
    }

    /// The same Run, launched in the Projet `project_id` (API 0.16).
    pub fn in_project(mut self, project_id: impl Into<String>) -> Self {
        self.project_id = Some(project_id.into());
        self
    }

    /// The same Run, named `name`.
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// The same Run, carrying `value` for the extension `extension`.
    pub fn with_extension(mut self, extension: impl Into<String>, value: Value) -> Self {
        self.extensions.insert(extension.into(), value);
        self
    }

    /// The same Run, coming from `provenance`: the core shows it as an
    /// "External source" ([`Provenance`]).
    pub fn from_source(mut self, provenance: Provenance) -> Self {
        self.provenance = Some(provenance);
        self
    }

    /// The body of the `POST /runs` this Run is: `pipeline`, `input`,
    /// `target_repo` (the primary), `source_branch` (the primary's branch),
    /// `target_repos` (every repository, only with a secondary), `name`,
    /// `project_id` (only in a Projet), `extensions` (only when it carries
    /// some). Never its [`Provenance`], which no `POST /runs` takes.
    pub fn to_request_body(&self) -> Value {
        let mut body = serde_json::json!({
            "pipeline": self.pipeline,
            "input": self.input,
        });
        if let Some(primary) = self.repos.first() {
            body["target_repo"] = Value::from(primary.repo.clone());
            if let Some(branch) = &primary.base_branch {
                body["source_branch"] = Value::from(branch.clone());
            }
        }
        if self.repos.len() > 1 {
            body["target_repos"] = serde_json::to_value(&self.repos).unwrap_or_default();
        }
        if let Some(name) = &self.name {
            body["name"] = Value::from(name.clone());
        }
        if let Some(project_id) = &self.project_id {
            body["project_id"] = Value::from(project_id.clone());
        }
        if !self.extensions.is_empty() {
            body["extensions"] = serde_json::to_value(&self.extensions).unwrap_or_default();
        }
        body
    }
}

/// Why a Run was not created: the status and the body `POST /runs` would
/// have answered (`403` `operation_refused` for a refused Actor, `400` for an
/// unknown pipeline or a value an extension refused…), or `503` when the core
/// cannot create Runs (the [`NeutralHost`](crate::NeutralHost), a daemon
/// starting or stopping).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RunNotCreated {
    pub status: u16,
    /// The body's `error` sentence (its `message` when it has one).
    pub message: String,
    /// The whole body, as `POST /runs` answers it.
    pub body: Value,
}

impl RunNotCreated {
    /// The refusal of status `status`, with the JSON body `body`.
    pub fn new(status: u16, body: Value) -> Self {
        let message = body
            .get("message")
            .or_else(|| body.get("error"))
            .and_then(Value::as_str)
            .unwrap_or("the Run was not created")
            .to_string();
        Self {
            status,
            message,
            body,
        }
    }

    /// `503`: this host cannot create a Run.
    pub fn unavailable(why: impl Into<String>) -> Self {
        Self::new(503, serde_json::json!({ "error": why.into() }))
    }
}

impl fmt::Display for RunNotCreated {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "run not created ({}): {}", self.status, self.message)
    }
}

impl std::error::Error for RunNotCreated {}

/// The answer of [`Host::create_run`](crate::Host::create_run): the new Run's
/// id once it exists, or why it does not. Await it from a route or a task;
/// from a thread, drive it with `tokio::runtime::Handle::block_on`.
pub type RunCreation = Pin<Box<dyn Future<Output = Result<String, RunNotCreated>> + Send>>;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_new_run_is_the_body_of_post_runs() {
        let run = NewRun::new("relay", "go")
            .in_repo(RunRepo::new("/repo").from_branch("main"))
            .named("Nightly")
            .with_extension("test", json!({ "cle": "valeur" }));
        assert_eq!(
            run.to_request_body(),
            json!({
                "pipeline": "relay",
                "input": "go",
                "target_repo": "/repo",
                "source_branch": "main",
                "name": "Nightly",
                "extensions": { "test": { "cle": "valeur" } },
            })
        );
    }

    #[test]
    fn a_run_launched_in_a_projet_names_it() {
        let run = NewRun::new("relay", "go")
            .in_repo(RunRepo::new("/repo"))
            .in_project("prj-1");
        assert_eq!(run.to_request_body()["project_id"], "prj-1");
    }

    #[test]
    fn secondaries_ride_in_target_repos_with_the_primary_first() {
        let run = NewRun::new("relay", "")
            .in_repo(RunRepo::new("/a"))
            .in_repo(RunRepo::new("/b").read_only());
        assert_eq!(
            run.to_request_body(),
            json!({
                "pipeline": "relay",
                "input": "",
                "target_repo": "/a",
                "target_repos": [{ "repo": "/a" }, { "repo": "/b", "read_only": true }],
            })
        );
        assert_eq!(
            NewRun::new("relay", "x").to_request_body(),
            json!({ "pipeline": "relay", "input": "x" }),
            "no repository, no name, no data: the bare request"
        );
    }

    #[test]
    fn a_provenance_rides_beside_the_body_never_in_it() {
        let run = NewRun::new("relay", "go").from_source(Provenance::new("Demo", "#/ext/test/x"));
        assert_eq!(
            run.provenance,
            Some(Provenance::new("Demo", "#/ext/test/x"))
        );
        assert_eq!(
            run.to_request_body(),
            json!({ "pipeline": "relay", "input": "go" })
        );
    }

    #[test]
    fn a_provenance_needs_a_label_and_a_deep_link_of_the_interface() {
        assert_eq!(
            Provenance::new("Demo", "#/ext/test/topbar/end/x").refusal(),
            None
        );
        assert_eq!(Provenance::new("⚡ Demo", "#/runs/abc").refusal(), None);
        assert!(Provenance::new("  ", "#/ext/test").refusal().is_some());
        for link in ["#/settings", "#/", "#/ext/", "#/runs/"] {
            assert!(Provenance::new("Demo", link).refusal().is_some(), "{link}");
        }
        assert!(Provenance::new("a\nb", "#/ext/test").refusal().is_some());
        assert!(Provenance::new("x".repeat(81), "#/ext/test")
            .refusal()
            .is_some());
        assert_eq!(
            Provenance::new("x".repeat(80), "#/ext/test").refusal(),
            None
        );
        for link in [
            "javascript:alert(1)",
            "https://example.com",
            "/ext/test",
            "#/a b",
            "",
        ] {
            assert!(Provenance::new("Demo", link).refusal().is_some(), "{link}");
        }
    }

    #[test]
    fn a_refusal_reads_its_sentence_from_the_body() {
        let refused = RunNotCreated::new(
            403,
            json!({ "error": "operation_refused", "message": "operation refused: no", "reason": "no" }),
        );
        assert_eq!(refused.message, "operation refused: no");
        let invalid = RunNotCreated::new(400, json!({ "error": "unknown pipeline" }));
        assert_eq!(invalid.message, "unknown pipeline");
        assert_eq!(
            invalid.to_string(),
            "run not created (400): unknown pipeline"
        );
        assert_eq!(RunNotCreated::unavailable("no daemon").status, 503);
    }
}
