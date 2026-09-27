//! The resolver (ADR-0077 §2): the absorptions of one Stats read, applied
//! where each fold reads a Run's identity, BEFORE aggregating.

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use serde_json::Value;

use super::identity::{node_defs, pipeline_identity, RunIdentity};
use super::tallies::{AbsorbedMember, PipelineTallies};
use super::{
    couple_key, effort_key, effort_name, effort_of_key, split_couple_key, AbsorptionRow, Named,
    COUPLE, EFFORT, MODEL, NODE, PIPELINE,
};

/// One dimension's absorptions within one scope: member → absorbent, the name
/// stored for each absorbent at its last Combine, and each absorbent's members.
#[derive(Debug, Clone, Default)]
pub struct ScopeAbsorptions {
    pub absorbent_of: HashMap<String, String>,
    pub absorbent_names: HashMap<String, String>,
    pub members: BTreeMap<String, Vec<Named>>,
}

impl ScopeAbsorptions {
    pub fn insert(&mut self, row: &AbsorptionRow) {
        self.absorbent_of
            .insert(row.member.clone(), row.absorbent.clone());
        self.absorbent_names
            .insert(row.absorbent.clone(), row.absorbent_name.clone());
        self.members
            .entry(row.absorbent.clone())
            .or_default()
            .push(Named {
                key: row.member.clone(),
                name: row.member_name.clone(),
            });
    }

    /// The key a row of `key` is counted under.
    pub fn resolve<'a>(&'a self, key: &'a str) -> &'a str {
        self.absorbent_of
            .get(key)
            .map(String::as_str)
            .unwrap_or(key)
    }

    pub fn stored_name(&self, key: &str) -> Option<&str> {
        self.absorbent_names.get(key).map(String::as_str)
    }

    pub fn members_of(&self, absorbent: &str) -> &[Named] {
        self.members
            .get(absorbent)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// The members of `absorbent` as its row lists them, with what each did in
    /// the period — every member, including one with no Run in it.
    pub fn absorbed(&self, absorbent: &str, tallies: &PipelineTallies) -> Vec<AbsorbedMember> {
        self.members
            .get(absorbent)
            .into_iter()
            .flatten()
            .map(|member| tallies.member(member))
            .collect()
    }
}

/// The couple absorptions of one raw `(Pipeline, Node)` pair (#906), their
/// keys resolved through the global absorptions — step 4 runs on couples the
/// global steps already resolved, so a global absorption posed later carries
/// a local one along (ADR-0078 §2).
#[derive(Debug, Clone, Default)]
pub struct CoupleScope {
    /// Resolved member couple → resolved absorbent couple.
    pub to: HashMap<String, String>,
    /// Each stored member, with its resolved key and its resolved absorbent's.
    pub members: Vec<(Named, String, String)>,
}

/// The global absorptions a couple goes through (steps 1–2): the efforts of a
/// raw model, then the model.
#[derive(Debug, Clone, Default)]
pub struct GlobalAbsorptions {
    pub models: Arc<ScopeAbsorptions>,
    /// Raw model id → its effort absorptions.
    pub efforts: Arc<HashMap<String, ScopeAbsorptions>>,
}

impl GlobalAbsorptions {
    pub fn effort(&self, raw_model: &str, raw_effort: Option<&str>) -> Option<String> {
        let key = effort_key(raw_effort);
        let resolved = self
            .efforts
            .get(raw_model)
            .map(|scope| scope.resolve(&key).to_string())
            .unwrap_or(key);
        effort_of_key(&resolved)
    }

    pub fn couple(&self, raw_model: &str, raw_effort: Option<&str>) -> (String, Option<String>) {
        (
            self.models.resolve(raw_model).to_string(),
            self.effort(raw_model, raw_effort),
        )
    }

    /// A stored couple key, resolved as if it were a raw couple.
    pub fn couple_key(&self, key: &str) -> Option<String> {
        let (model, effort) = split_couple_key(key)?;
        let (model, effort) = self.couple(&model, effort.as_deref());
        Some(couple_key(&model, effort.as_deref()))
    }
}

/// The absorptions of one Stats read. Empty under `uncombined=true`, which is
/// exactly « the rows as the event log wrote them ».
#[derive(Debug, Clone, Default)]
pub struct AbsorptionResolver {
    pub pipelines: ScopeAbsorptions,
    /// Pipeline key (of the row the Node shows under) → its Node absorptions.
    pub nodes: HashMap<String, Arc<ScopeAbsorptions>>,
    pub global: GlobalAbsorptions,
    /// [`couple_scope`] of a raw `(Pipeline, Node)` → its couple absorptions.
    pub couples: Arc<HashMap<String, CoupleScope>>,
    pub fingerprint: u64,
}

impl AbsorptionResolver {
    pub fn from_rows(rows: &[AbsorptionRow]) -> Self {
        let mut resolver = Self::default();
        let mut hasher = DefaultHasher::new();
        let mut sorted: Vec<&AbsorptionRow> = rows.iter().collect();
        sorted.sort_by(|a, b| {
            (&a.dimension, &a.scope, &a.member).cmp(&(&b.dimension, &b.scope, &b.member))
        });
        let mut nodes: HashMap<String, ScopeAbsorptions> = HashMap::new();
        let mut models = ScopeAbsorptions::default();
        let mut efforts: HashMap<String, ScopeAbsorptions> = HashMap::new();
        let mut couple_rows: Vec<&AbsorptionRow> = Vec::new();
        for row in sorted {
            (
                &row.dimension,
                &row.scope,
                &row.member,
                &row.member_name,
                &row.absorbent,
                &row.absorbent_name,
            )
                .hash(&mut hasher);
            match row.dimension.as_str() {
                PIPELINE => resolver.pipelines.insert(row),
                NODE => nodes.entry(row.scope.clone()).or_default().insert(row),
                MODEL => models.insert(row),
                EFFORT => efforts.entry(row.scope.clone()).or_default().insert(row),
                COUPLE => couple_rows.push(row),
                _ => {}
            }
        }
        resolver.nodes = nodes
            .into_iter()
            .map(|(scope, absorptions)| (scope, Arc::new(absorptions)))
            .collect();
        resolver.global = GlobalAbsorptions {
            models: Arc::new(models),
            efforts: Arc::new(efforts),
        };
        let mut couples: HashMap<String, CoupleScope> = HashMap::new();
        for row in couple_rows {
            let (Some(member), Some(absorbent)) = (
                resolver.global.couple_key(&row.member),
                resolver.global.couple_key(&row.absorbent),
            ) else {
                continue;
            };
            let scope = couples.entry(row.scope.clone()).or_default();
            if member != absorbent {
                scope
                    .to
                    .entry(member.clone())
                    .or_insert_with(|| absorbent.clone());
            }
            scope.members.push((
                Named {
                    key: row.member.clone(),
                    name: row.member_name.clone(),
                },
                member,
                absorbent,
            ));
        }
        resolver.couples = Arc::new(couples);
        resolver.fingerprint = hasher.finish();
        resolver
    }

    /// The resolver of one request: every stored absorption, or none at all
    /// under `uncombined`.
    pub fn for_read(rows: &[AbsorptionRow], uncombined: bool) -> Self {
        if uncombined {
            return Self::default();
        }
        Self::from_rows(rows)
    }

    /// The key a Run of Pipeline `key` is counted under.
    pub fn pipeline<'a>(&'a self, key: &'a str) -> &'a str {
        self.pipelines.resolve(key)
    }

    /// The name stored for an absorbent key at its last Combine or rename — for
    /// a row that has no Run of its own to name it.
    pub fn stored_name(&self, key: &str) -> Option<&str> {
        self.pipelines.stored_name(key)
    }

    /// Folded into every cache that keeps an already-aggregated result (ADR-0077
    /// §2): a Combine or an Uncombine changes it.
    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }

    /// The Run identity of one `run_started` payload, absorptions applied.
    pub fn run_identity(&self, payload: &Value) -> RunIdentity {
        let (raw_key, raw_name) = pipeline_identity(payload);
        let key = self.pipeline(&raw_key).to_string();
        let own = key == raw_key;
        let name = if own {
            raw_name.clone()
        } else {
            self.pipelines
                .stored_name(&key)
                .map(str::to_string)
                .unwrap_or_else(|| key.clone())
        };
        RunIdentity {
            nodes: self.nodes.get(&key).cloned().unwrap_or_default(),
            global: self.global.clone(),
            couples: self.couples.clone(),
            pipeline_key: key,
            pipeline_name: name,
            own_name: own,
            raw_pipeline_key: raw_key,
            node_defs: node_defs(payload),
            run_id: String::new(),
            started_at: String::new(),
        }
    }

    /// The members of Pipeline `absorbent` as its row lists them.
    pub fn absorbed(&self, absorbent: &str, tallies: &PipelineTallies) -> Vec<AbsorbedMember> {
        self.pipelines.absorbed(absorbent, tallies)
    }

    /// The members of Node `absorbent` under Pipeline row `pipeline`.
    pub fn absorbed_nodes(
        &self,
        pipeline: &str,
        absorbent: &str,
        tallies: &PipelineTallies,
    ) -> Vec<AbsorbedMember> {
        self.nodes
            .get(pipeline)
            .map(|scope| scope.absorbed(absorbent, tallies))
            .unwrap_or_default()
    }

    /// The members of model `absorbent` on the « By model » axis.
    pub fn absorbed_models(
        &self,
        absorbent: &str,
        tallies: &PipelineTallies,
    ) -> Vec<AbsorbedMember> {
        self.global.models.absorbed(absorbent, tallies)
    }

    /// The members of effort `absorbent` (its key, `""` for « not set ») under
    /// a model row that counts the raw model ids `raw_models` (#906). Each
    /// member lists the raw models whose absorption holds it — its ✕ takes it
    /// out of all of them. `tallies` are keyed by raw effort key.
    pub fn absorbed_efforts<'a>(
        &self,
        raw_models: impl IntoIterator<Item = &'a str>,
        absorbent: &str,
        tallies: &PipelineTallies,
    ) -> Vec<AbsorbedMember> {
        let mut members: BTreeMap<String, AbsorbedMember> = BTreeMap::new();
        for raw_model in raw_models {
            let Some(scope) = self.global.efforts.get(raw_model) else {
                continue;
            };
            for member in scope.members_of(absorbent) {
                members
                    .entry(member.key.clone())
                    .or_insert_with(|| AbsorbedMember {
                        name: effort_name(effort_of_key(&member.key).as_deref()),
                        ..tallies.member(member)
                    })
                    .scopes
                    .push(raw_model.to_string());
            }
        }
        members.into_values().collect()
    }

    /// The members of couple `absorbent` of a Node row that counts the raw
    /// `(Pipeline, Node)` scopes `scopes` (#906), each with the scopes that hold
    /// it. `tallies` are keyed by globally resolved couple key.
    pub fn absorbed_couples<'a>(
        &self,
        scopes: impl IntoIterator<Item = &'a String>,
        absorbent: &str,
        tallies: &PipelineTallies,
    ) -> Vec<AbsorbedMember> {
        let mut members: BTreeMap<String, AbsorbedMember> = BTreeMap::new();
        for scope_key in scopes {
            let Some(scope) = self.couples.get(scope_key) else {
                continue;
            };
            for (member, resolved, resolved_absorbent) in &scope.members {
                if resolved_absorbent != absorbent {
                    continue;
                }
                members
                    .entry(member.key.clone())
                    .or_insert_with(|| AbsorbedMember {
                        key: member.key.clone(),
                        name: member.name.clone(),
                        ..tallies.member(&Named {
                            key: resolved.clone(),
                            name: member.name.clone(),
                        })
                    })
                    .scopes
                    .push(scope_key.clone());
            }
        }
        members.into_values().collect()
    }
}
