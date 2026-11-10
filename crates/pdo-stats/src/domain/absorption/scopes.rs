//! What a write covers beyond its own rows, read off the stored absorptions and
//! the Pipelines' history:
//!
//! - the **frozen scope** of an effort or couple absorption (ADR-0078): the raw
//!   keys it covers when it is posed;
//! - the **cascade** of taking a Pipeline out of its absorption (#892, grilling
//!   Q13): the Nodes of its former absorbent's scope that leave with it.
//!
//! Which Pipeline a Node ran under is read off the event log, never guessed:
//! the caller hands each raw Pipeline's [`PipelineHistory`] in.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::identity::{node_defs, pipeline_identity};
use super::{couple_scope, parse_couple_scope, AbsorptionRow, Named, MODEL, NODE, PIPELINE};

/// What the event log says of one raw Pipeline key, the whole history.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PipelineHistory {
    /// The Node ids that started under one of its Runs.
    pub ran: BTreeSet<String>,
    /// The `run_started` payload of its most recent Run.
    pub latest: Option<Value>,
}

/// The members `absorbent` holds in `dimension` × `scope`, by key.
pub fn members_of(
    rows: &[AbsorptionRow],
    dimension: &str,
    scope: &str,
    absorbent: &str,
) -> Vec<String> {
    let mut members: Vec<String> = rows
        .iter()
        .filter(|row| {
            row.dimension == dimension && row.scope == scope && row.absorbent == absorbent
        })
        .map(|row| row.member.clone())
        .collect();
    members.sort();
    members
}

/// The raw model ids an effort absorption posed on model row `scope` covers:
/// the row's own id and the models it absorbs now.
pub fn effort_cover(rows: &[AbsorptionRow], scope: &str) -> Vec<Named> {
    let mut covered = vec![Named {
        key: scope.to_string(),
        name: scope.to_string(),
    }];
    for model in members_of(rows, MODEL, "", scope) {
        covered.push(Named {
            key: model.clone(),
            name: model,
        });
    }
    covered
}

/// The raw Pipelines and Nodes a couple absorption posed on Node row `scope`
/// crosses: the row's Pipeline and the Pipelines it absorbs, the row's Node and
/// the Nodes it absorbs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoupleCover {
    pub pipeline: String,
    pub node: String,
    pub pipelines: Vec<String>,
    pub nodes: Vec<String>,
}

/// The [`CoupleCover`] of a couple scope, `None` when the scope names no
/// `(Pipeline, Node)` pair.
pub fn couple_cover(rows: &[AbsorptionRow], scope: &str) -> Option<CoupleCover> {
    let (pipeline, node) = parse_couple_scope(scope)?;
    let mut pipelines = vec![pipeline.clone()];
    pipelines.extend(members_of(rows, PIPELINE, "", &pipeline));
    let mut nodes = vec![node.clone()];
    nodes.extend(members_of(rows, NODE, &pipeline, &node));
    Some(CoupleCover {
        pipeline,
        node,
        pipelines,
        nodes,
    })
}

/// The raw `(Pipeline, Node)` scopes of a [`CoupleCover`], kept where the Node
/// really ran, each named « Node (Pipeline) » (the row's own scope keeps
/// `scope_name` when there is one).
pub fn covered_couples(
    cover: &CoupleCover,
    scope_name: &str,
    histories: &BTreeMap<String, PipelineHistory>,
) -> Vec<Named> {
    let mut covered = Vec::new();
    for raw_pipeline in &cover.pipelines {
        let history = histories.get(raw_pipeline);
        for raw_node in &cover.nodes {
            let own = raw_pipeline == &cover.pipeline && raw_node == &cover.node;
            if !own && !history.is_some_and(|h| h.ran.contains(raw_node)) {
                continue;
            }
            let name = if own && !scope_name.is_empty() {
                scope_name.to_string()
            } else {
                node_label(
                    raw_pipeline,
                    raw_node,
                    history.and_then(|h| h.latest.as_ref()),
                )
            };
            covered.push(Named {
                key: couple_scope(raw_pipeline, raw_node),
                name,
            });
        }
    }
    covered
}

/// « Node (Pipeline) », by the names the most recent Run of `pipeline` froze.
pub fn node_label(pipeline: &str, node: &str, latest: Option<&Value>) -> String {
    let payload = latest.cloned().unwrap_or(Value::Null);
    let (_, pipeline_name) = if payload.is_null() {
        (pipeline.to_string(), pipeline.to_string())
    } else {
        pipeline_identity(&payload)
    };
    let node_name = node_defs(&payload)
        .get(node)
        .map(|def| def.name.clone())
        .unwrap_or_else(|| node.to_string());
    format!("{node_name} ({pipeline_name})")
}

/// The Pipelines still counted in `absorbent`'s row once `removed` leaves it.
pub fn remaining_pipelines(rows: &[AbsorptionRow], removed: &str, absorbent: &str) -> Vec<String> {
    members_of(rows, PIPELINE, "", absorbent)
        .into_iter()
        .filter(|member| member != removed)
        .collect()
}

/// The cascade of taking Pipeline `removed` out of `absorbent`'s row: the
/// members of the Node absorptions scoped to `absorbent` that leave. A Node
/// leaves when it ran under `removed` (`gone`) and under none of the Pipelines
/// still counted in the row (`staying`); a member leaves with it, and so does
/// every member of a Node absorbent that leaves — they find their own rows
/// again.
pub fn released_nodes(
    rows: &[AbsorptionRow],
    absorbent: &str,
    gone: &BTreeSet<String>,
    staying: &BTreeSet<String>,
) -> Vec<String> {
    let leaves = |node: &str| gone.contains(node) && !staying.contains(node);
    rows.iter()
        .filter(|row| row.dimension == NODE && row.scope == absorbent)
        .filter(|row| leaves(&row.member) || leaves(&row.absorbent))
        .map(|row| row.member.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(dimension: &str, scope: &str, member: &str, absorbent: &str) -> AbsorptionRow {
        AbsorptionRow {
            dimension: dimension.into(),
            scope: scope.into(),
            scope_name: String::new(),
            member: member.into(),
            member_name: member.into(),
            absorbent: absorbent.into(),
            absorbent_name: absorbent.into(),
            origin: "manual".into(),
            created_at: String::new(),
        }
    }

    fn ran(nodes: &[&str]) -> PipelineHistory {
        PipelineHistory {
            ran: nodes.iter().map(|n| n.to_string()).collect(),
            latest: None,
        }
    }

    #[test]
    fn a_couple_covers_its_row_crossed_with_its_members_where_they_ran() {
        let rows = [
            stored(PIPELINE, "", "old", "p"),
            stored(NODE, "p", "review", "coder"),
        ];
        let cover = couple_cover(&rows, &couple_scope("p", "coder")).unwrap();
        assert_eq!(cover.pipelines, ["p", "old"]);
        assert_eq!(cover.nodes, ["coder", "review"]);
        let histories = BTreeMap::from([
            ("p".to_string(), ran(&["coder"])),
            ("old".to_string(), ran(&["review"])),
        ]);
        let covered = covered_couples(&cover, "Coder (P)", &histories);
        assert_eq!(
            covered.iter().map(|n| n.key.clone()).collect::<Vec<_>>(),
            [couple_scope("p", "coder"), couple_scope("old", "review")]
        );
        assert_eq!(covered[0].name, "Coder (P)");
        assert_eq!(covered[1].name, "review (old)");
    }

    #[test]
    fn a_node_label_reads_the_latest_runs_frozen_names() {
        let latest = serde_json::json!({
            "pipeline_id": "p",
            "pipeline_name": "Pipeline",
            "node_defs": [{"id": "n", "name": "Node"}],
        });
        assert_eq!(node_label("p", "n", Some(&latest)), "Node (Pipeline)");
        assert_eq!(node_label("p", "n", None), "n (p)");
    }

    #[test]
    fn a_pipeline_taken_out_releases_the_nodes_only_it_ran() {
        let rows = [
            stored(PIPELINE, "", "gone", "p"),
            stored(PIPELINE, "", "kept", "p"),
            stored(NODE, "p", "only-gone", "shared"),
            stored(NODE, "p", "member", "only-gone-absorbent"),
            stored(NODE, "p", "everywhere", "shared"),
        ];
        assert_eq!(remaining_pipelines(&rows, "gone", "p"), ["kept"]);
        let gone = BTreeSet::from([
            "only-gone".to_string(),
            "only-gone-absorbent".to_string(),
            "everywhere".to_string(),
        ]);
        let staying = BTreeSet::from(["everywhere".to_string(), "shared".to_string()]);
        assert_eq!(
            released_nodes(&rows, "p", &gone, &staying),
            ["only-gone", "member"]
        );
        assert_eq!(
            effort_cover(&[stored(MODEL, "", "m-old", "m")], "m").len(),
            2
        );
    }
}
