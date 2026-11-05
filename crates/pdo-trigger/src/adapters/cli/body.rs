//! The bodies `pdo trigger create` and `update` send: the Run template's
//! fields, as the routes name them.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use clap::{Args, ValueEnum};
use pdo_platform::adapters::cli::daemon::{absolute_path, agent_choice};
use serde_json::{json, Map, Value};

/// The fields of the Run template `create` and `update` share.
#[derive(Args, Debug, Default)]
pub struct TemplateFields {
    /// The repository the fired Runs work in (made absolute).
    #[arg(long)]
    pub repo: Option<String>,
    /// The Projet the fired Runs are launched in, holding `--repo`.
    #[arg(long)]
    pub project: Option<String>,
    /// The branch the fired Runs start from.
    #[arg(long)]
    pub source_branch: Option<String>,
    /// The prompt of the fired Runs (`{{var}}` reads `--var`, the guard's
    /// output fills `{{guard_output}}`).
    #[arg(long, conflicts_with = "input_file")]
    pub input: Option<String>,
    /// The prompt, read from a file.
    #[arg(long, value_name = "FILE")]
    pub input_file: Option<String>,
    /// A variable of the template, `key=value` (repeatable).
    #[arg(long = "var", value_name = "KEY=VALUE")]
    pub vars: Vec<String>,
    /// A guard command: a due tick fires only when it exits 0.
    #[arg(long)]
    pub guard: Option<String>,
    /// What a due tick does while a fired Run is still live.
    #[arg(long, value_enum)]
    pub overlap: Option<Overlap>,
    /// With `--overlap allow`: at most this many live Runs.
    #[arg(long)]
    pub max_concurrent: Option<i64>,
    /// The sandbox mode of the fired Runs (`off`, `full`, `minimal`, or a
    /// sandbox profile).
    #[arg(long)]
    pub sandbox: Option<String>,
    /// The harness of the fired Runs.
    #[arg(long)]
    pub harness: Option<String>,
    /// The agentic profile of the fired Runs (a profile id).
    #[arg(long, conflicts_with = "agent_harness")]
    pub profile: Option<String>,
    /// An inline combination instead of a profile: this harness, with
    /// `--model` and `--effort`.
    #[arg(long)]
    pub agent_harness: Option<String>,
    #[arg(long, requires = "agent_harness")]
    pub model: Option<String>,
    #[arg(long, requires = "agent_harness")]
    pub effort: Option<String>,
    /// A secondary repository the fired Runs also work in, `PATH` or
    /// `PATH=BRANCH` (repeatable); replaces the list on update.
    #[arg(long = "secondary-repo", value_name = "PATH[=BRANCH]")]
    pub secondary_repos: Vec<String>,
    /// A secondary repository the fired Runs read without writing to it,
    /// `PATH` or `PATH=BRANCH` (repeatable).
    #[arg(long = "read-only-repo", value_name = "PATH[=BRANCH]")]
    pub read_only_repos: Vec<String>,
    /// A skill the fired Runs carry, `ID` or `ID=LABEL` (repeatable);
    /// replaces the list on update.
    #[arg(long = "skill", value_name = "ID[=LABEL]")]
    pub skills: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Overlap {
    Skip,
    Allow,
}

/// The template fields given, as the create and patch bodies name them.
pub(super) fn template_body(fields: TemplateFields) -> Result<Map<String, Value>> {
    let mut body = Map::new();
    let input = match (fields.input, fields.input_file) {
        (Some(input), _) => Some(input),
        (None, Some(file)) => {
            Some(std::fs::read_to_string(&file).with_context(|| format!("cannot read {file}"))?)
        }
        (None, None) => None,
    };
    let repo = fields.repo.as_deref().map(absolute_path).transpose()?;
    for (key, value) in [
        ("target_repo", repo),
        ("project_id", fields.project),
        ("source_branch", fields.source_branch),
        ("input_template", input),
        ("guard_command", fields.guard),
        ("sandbox", fields.sandbox),
        ("harness", fields.harness),
    ] {
        if let Some(value) = value {
            body.insert(key.into(), json!(value));
        }
    }
    if let Some(overlap) = fields.overlap {
        let policy = match overlap {
            Overlap::Skip => "skip",
            Overlap::Allow => "allow",
        };
        body.insert("overlap_policy".into(), json!(policy));
    }
    if let Some(max) = fields.max_concurrent {
        body.insert("max_concurrent".into(), json!(max));
    }
    if !fields.vars.is_empty() {
        body.insert("variables".into(), json!(variables(&fields.vars)?));
    }
    if !fields.skills.is_empty() {
        let skills: Vec<Value> = fields
            .skills
            .iter()
            .map(|s| {
                let (id, label) = s.split_once('=').unwrap_or((s, s));
                json!({ "id": id, "name": label })
            })
            .collect();
        body.insert("skills".into(), json!(skills));
    }
    if let Some(choice) = agent_choice(
        fields.profile,
        fields.agent_harness,
        fields.model,
        fields.effort,
    ) {
        body.insert("agent_choice".into(), choice);
    }
    if !fields.secondary_repos.is_empty() || !fields.read_only_repos.is_empty() {
        let mut repos = Vec::new();
        for (raw, read_only) in fields
            .secondary_repos
            .iter()
            .map(|r| (r, false))
            .chain(fields.read_only_repos.iter().map(|r| (r, true)))
        {
            repos.push(secondary_repo(raw, read_only)?);
        }
        body.insert("target_repos".into(), json!(repos));
    }
    Ok(body)
}

/// `PATH` or `PATH=BRANCH` as a secondary repository of the route.
fn secondary_repo(raw: &str, read_only: bool) -> Result<Value> {
    let (path, branch) = match raw.split_once('=') {
        Some((path, branch)) => (path, Some(branch.trim()).filter(|b| !b.is_empty())),
        None => (raw, None),
    };
    let mut repo = json!({ "repo": absolute_path(path)? });
    if let Some(branch) = branch {
        repo["base_branch"] = json!(branch);
    }
    if read_only {
        repo["read_only"] = json!(true);
    }
    Ok(repo)
}

pub(super) fn variables(raw: &[String]) -> Result<BTreeMap<String, String>> {
    raw.iter()
        .map(|pair| {
            let (key, value) = pair
                .split_once('=')
                .with_context(|| format!("--var {pair}: expected KEY=VALUE"))?;
            let key = key.trim();
            anyhow::ensure!(!key.is_empty(), "--var {pair}: the key is empty");
            Ok((key.to_string(), value.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_template_flags_name_the_route_s_fields() {
        let body = template_body(TemplateFields {
            repo: Some("/srv/web".into()),
            input: Some("go".into()),
            vars: vec!["env=prod".into(), "n=a=b".into()],
            overlap: Some(Overlap::Allow),
            max_concurrent: Some(2),
            skills: vec!["tdd".into()],
            ..TemplateFields::default()
        })
        .unwrap();
        assert_eq!(
            Value::Object(body),
            json!({
                "target_repo": "/srv/web",
                "input_template": "go",
                "variables": { "env": "prod", "n": "a=b" },
                "overlap_policy": "allow",
                "max_concurrent": 2,
                "skills": [{ "id": "tdd", "name": "tdd" }]
            })
        );
        assert!(template_body(TemplateFields::default()).unwrap().is_empty());
    }

    #[test]
    fn secondary_repos_labels_and_an_agent_travel_as_the_form_sends_them() {
        let body = template_body(TemplateFields {
            secondary_repos: vec!["/srv/api=develop".into()],
            read_only_repos: vec!["/srv/docs/".into()],
            skills: vec!["tdd=TDD".into()],
            profile: Some("reviewer".into()),
            ..TemplateFields::default()
        })
        .unwrap();
        assert_eq!(
            Value::Object(body),
            json!({
                "target_repos": [
                    { "repo": "/srv/api", "base_branch": "develop" },
                    { "repo": "/srv/docs", "read_only": true }
                ],
                "skills": [{ "id": "tdd", "name": "TDD" }],
                "agent_choice": { "mode": "profile", "profile_id": "reviewer" }
            })
        );
    }

    #[test]
    fn a_variable_without_a_key_is_refused() {
        assert!(variables(&["novalue".into()]).is_err());
        assert!(variables(&["=x".into()]).is_err());
    }
}
