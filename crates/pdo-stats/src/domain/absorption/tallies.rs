//! What a row says about its members: the per-raw-key tallies of a row, and
//! each absorbed member as the row lists it.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use ts_rs::TS;

use super::identity::RunIdentity;
use super::{couple_name, split_couple_key, Named};
use crate::domain::provenance::{provenance, StatsProvenance};

/// What one raw key did in the period, under the row that counts it.
#[derive(Debug, Clone, Default)]
pub struct KeyTally {
    pub runs: BTreeSet<String>,
    pub executions: u64,
    pub last_run: Option<String>,
    pub observed: u64,
    pub requested: u64,
}

/// The per-raw-key tallies of one row (a Pipeline, a Node or a model): the
/// row's own `runs` / `last_run`, and what each absorbed member contributed.
#[derive(Debug, Clone, Default)]
pub struct PipelineTallies {
    pub by_key: BTreeMap<String, KeyTally>,
    pub count_executions: bool,
}

impl PipelineTallies {
    /// Sessions counts executions; Cost and Performance count Runs only.
    pub fn counting_executions() -> Self {
        Self {
            count_executions: true,
            ..Self::default()
        }
    }

    pub fn record_run(&mut self, identity: &RunIdentity, run_id: &str, started_at: &str) {
        self.record_run_as(&identity.raw_pipeline_key, run_id, started_at);
    }

    pub fn record_execution(&mut self, identity: &RunIdentity) {
        self.record_execution_as(&identity.raw_pipeline_key);
    }

    /// A Run seen under raw key `key` (a Pipeline key, a Node id or a model id).
    pub fn record_run_as(&mut self, key: &str, run_id: &str, started_at: &str) {
        let tally = self.by_key.entry(key.to_string()).or_default();
        tally.runs.insert(run_id.to_string());
        if tally
            .last_run
            .as_deref()
            .is_none_or(|last| last < started_at)
        {
            tally.last_run = Some(started_at.to_string());
        }
    }

    pub fn record_execution_as(&mut self, key: &str) {
        self.by_key.entry(key.to_string()).or_default().executions += 1;
    }

    /// One observation of model `key`, read from the source or requested — only
    /// model rows record it (#892), so only their members carry a provenance.
    pub fn record_provenance_as(&mut self, key: &str, observed: bool) {
        let tally = self.by_key.entry(key.to_string()).or_default();
        if observed {
            tally.observed += 1;
        } else {
            tally.requested += 1;
        }
    }

    pub fn runs(&self) -> u64 {
        self.by_key
            .values()
            .flat_map(|t| t.runs.iter())
            .collect::<BTreeSet<_>>()
            .len() as u64
    }

    pub fn last_run(&self) -> Option<String> {
        self.by_key
            .values()
            .filter_map(|t| t.last_run.clone())
            .max()
    }

    /// The raw keys seen (a model row's raw model ids…).
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.by_key.keys().map(String::as_str)
    }

    /// The raw couples a couple row counts through a global absorption (#906),
    /// as its read-only « Global absorption » list shows them. `self` is keyed
    /// by raw couple key, and only records the moved ones.
    pub fn global_members(&self) -> Vec<AbsorbedMember> {
        self.by_key
            .keys()
            .map(|key| {
                let name = split_couple_key(key)
                    .map(|(model, effort)| couple_name(&model, effort.as_deref()))
                    .unwrap_or_else(|| key.clone());
                self.member(&Named {
                    key: key.clone(),
                    name,
                })
            })
            .collect()
    }

    pub fn member(&self, member: &Named) -> AbsorbedMember {
        let tally = self.by_key.get(&member.key);
        AbsorbedMember {
            key: member.key.clone(),
            name: member.name.clone(),
            runs: tally.map_or(0, |t| t.runs.len() as u64),
            executions: self
                .count_executions
                .then(|| tally.map_or(0, |t| t.executions)),
            last_run: tally.and_then(|t| t.last_run.clone()),
            provenance: tally
                .filter(|t| t.observed + t.requested > 0)
                .map(|t| provenance(t.observed as i64, t.requested as i64)),
            scopes: Vec::new(),
        }
    }
}

/// One absorbed member as a row lists it: its name, and what it did in the
/// period (`runs` always; `executions` on Sessions rows only; `provenance` on
/// model rows only — where its id was read from, ADR-0065 §1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "StatsAbsorbedMember")]
pub struct AbsorbedMember {
    pub key: String,
    pub name: String,
    pub runs: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub executions: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_run: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub provenance: Option<StatsProvenance>,
    /// The raw scopes whose absorption holds this member — on effort and
    /// couple rows only (#906): their ✕ takes it out of every one of them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[ts(optional = nullable)]
    pub scopes: Vec<String>,
}

// --- HTTP ----------------------------------------------------------------------
