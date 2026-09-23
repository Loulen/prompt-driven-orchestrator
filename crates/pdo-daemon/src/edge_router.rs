//! Conditional routing on edges (ADR-0011).
//!
//! When an artifact leaves a producing node's output port, **every** outgoing
//! edge whose `when:` clause is satisfied fires — multi-match fan-out, no
//! first-match ordering. An `else` edge fires iff **no sibling edge on the same
//! source port** matched. An edge with neither `when:` nor `else` is
//! unconditional and always fires.
//!
//! An edge may carry SEVERAL output ports of its source (ADR-0073 / #843) and is
//! still one edge here: it appears at most once in the fired set, and its verdict
//! settles every port it carries — so a match suppresses an `else` on any of
//! them, and a multi-port `else` fires iff none of its own ports routed.
//!
//! Reuses the mechanical predicate evaluator in [`crate::condition`] (ADR-0002):
//! conditions reference only `iter`, the source node's frontmatter fields, and
//! pipeline variables — never an LLM-router. A clause on a multi-port edge names
//! WHICH carried output it reads by qualifying the field (`spec.ready`); the
//! producer's frontmatter map carries both spellings (see
//! `resolve_source_frontmatter`), so an unqualified pre-#843 clause is unchanged.

use std::collections::HashMap;

use crate::condition;
use crate::pipeline::EdgeDef;

/// Returns the subset of `outgoing` edges that fire for the producing node,
/// in input order. `outgoing` must be exactly the edges whose `source.node` is
/// the producing node.
pub(crate) fn fired_edges<'a>(
    outgoing: &'a [&'a EdgeDef],
    frontmatter: &HashMap<String, serde_yaml::Value>,
    vars: &HashMap<String, serde_yaml::Value>,
    iter: i64,
) -> Vec<&'a EdgeDef> {
    let ctx = condition::EvalContext::new(iter)
        .with_fields(frontmatter.clone())
        .with_variables(vars.clone());

    // Evaluate each guard once and cache it: the `else` rule needs the whole
    // port's verdict, so a single pass would re-evaluate predicates.
    let mut guard_matched: Vec<bool> = Vec::with_capacity(outgoing.len());
    let mut matched_ports: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for edge in outgoing {
        let matched = edge
            .when
            .as_ref()
            .is_some_and(|when| condition::evaluate_with_iter(when, &ctx));
        if matched {
            // A multi-port edge (ADR-0073) settles the verdict for EVERY port it
            // carries: it is one edge, one firing. So each carried port counts as
            // matched, and an `else` sibling on any of them is suppressed.
            for port in &edge.source.ports {
                matched_ports.insert(port.as_str());
            }
        }
        guard_matched.push(matched);
    }

    let mut fired = Vec::new();
    for (edge, &matched) in outgoing.iter().zip(&guard_matched) {
        let fires = if edge.when.is_some() {
            matched
        } else if edge.is_else {
            // Symmetrically: a fallback edge fires iff NONE of the ports it
            // carries saw a sibling match. One carried port already routed is
            // enough to make it a non-default path.
            !edge
                .source
                .ports
                .iter()
                .any(|p| matched_ports.contains(p.as_str()))
        } else {
            true
        };
        if fires {
            fired.push(*edge);
        }
    }
    fired
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{EdgeDef, EdgeEndpoint, EdgeSource};
    use pretty_assertions::assert_eq;

    fn edge(src_port: &str, tgt: &str, when: Option<&str>, is_else: bool) -> EdgeDef {
        EdgeDef {
            source: EdgeSource::single("producer", src_port),
            target: EdgeEndpoint {
                node: tgt.into(),
                port: "in".into(),
            },
            reason: None,
            when: when.map(|s| serde_yaml::from_str(s).unwrap()),
            is_else,
            repeated: false,
            ..Default::default()
        }
    }

    fn val_s(s: &str) -> serde_yaml::Value {
        serde_yaml::Value::String(s.into())
    }

    fn fired_targets(
        edges: &[EdgeDef],
        fm: HashMap<String, serde_yaml::Value>,
        iter: i64,
    ) -> Vec<String> {
        let refs: Vec<&EdgeDef> = edges.iter().collect();
        fired_edges(&refs, &fm, &HashMap::new(), iter)
            .iter()
            .map(|e| e.target.node.clone())
            .collect()
    }

    #[test]
    fn unconditional_edge_always_fires() {
        let edges = vec![edge("out", "downstream", None, false)];
        assert_eq!(
            fired_targets(&edges, HashMap::new(), 1),
            vec!["downstream".to_string()]
        );
    }

    #[test]
    fn guarded_edge_fires_only_when_clause_matches() {
        let edges = vec![edge(
            "out",
            "implementer",
            Some("verdict: { eq: FAIL }"),
            false,
        )];

        let matched = [("verdict".to_string(), val_s("FAIL"))]
            .into_iter()
            .collect();
        assert_eq!(
            fired_targets(&edges, matched, 1),
            vec!["implementer".to_string()]
        );

        let unmatched = [("verdict".to_string(), val_s("PASS"))]
            .into_iter()
            .collect();
        assert!(fired_targets(&edges, unmatched, 1).is_empty());
    }

    #[test]
    fn overlapping_guarded_edges_both_fire_multi_match() {
        // No first-match ordering: an artifact satisfying two guarded edges
        // leaving the same port fans out to BOTH targets (ADR-0011).
        let edges = vec![
            edge("out", "hotfix", Some("severity: { eq: high }"), false),
            edge(
                "out",
                "security-review",
                Some("security: { eq: true }"),
                false,
            ),
        ];
        let fm = [
            ("severity".to_string(), val_s("high")),
            ("security".to_string(), serde_yaml::Value::Bool(true)),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            fired_targets(&edges, fm, 1),
            vec!["hotfix".to_string(), "security-review".to_string()]
        );
    }

    #[test]
    fn else_fires_iff_no_sibling_matched() {
        let edges = vec![
            edge("out", "implementer", Some("verdict: { eq: FAIL }"), false),
            edge("out", "archiver", None, true), // else
        ];

        let matched = [("verdict".to_string(), val_s("FAIL"))]
            .into_iter()
            .collect();
        assert_eq!(
            fired_targets(&edges, matched, 1),
            vec!["implementer".to_string()]
        );

        let none = [("verdict".to_string(), val_s("PASS"))]
            .into_iter()
            .collect();
        assert_eq!(fired_targets(&edges, none, 1), vec!["archiver".to_string()]);
    }

    #[test]
    fn else_is_scoped_to_its_own_source_port() {
        // A match on port `a` must NOT suppress an `else` leaving port `b`.
        let edges = vec![
            edge("a", "matched_a", Some("verdict: { eq: PASS }"), false),
            edge("b", "else_b", None, true),
        ];
        let fm = [("verdict".to_string(), val_s("PASS"))]
            .into_iter()
            .collect();
        assert_eq!(
            fired_targets(&edges, fm, 1),
            vec!["matched_a".to_string(), "else_b".to_string()]
        );
    }

    #[test]
    fn iter_is_referenceable_in_when() {
        // ADR-0011: `iter` is re-authorised in `when:` (e.g. exhaustion exits).
        let edges = vec![edge("out", "exhausted", Some("iter: { gte: 3 }"), false)];
        assert!(fired_targets(&edges, HashMap::new(), 2).is_empty());
        assert_eq!(
            fired_targets(&edges, HashMap::new(), 3),
            vec!["exhausted".to_string()]
        );
    }

    // ── Multi-output edges (ADR-0073 / #843) ────────────────────────────────

    fn multi_edge(ports: &[&str], tgt: &str, when: Option<&str>, is_else: bool) -> EdgeDef {
        EdgeDef {
            source: EdgeSource {
                node: "producer".into(),
                ports: ports.iter().map(|p| (*p).to_string()).collect(),
            },
            target: EdgeEndpoint {
                node: tgt.into(),
                port: "in".into(),
            },
            reason: None,
            when: when.map(|s| serde_yaml::from_str(s).unwrap()),
            is_else,
            repeated: false,
            ..Default::default()
        }
    }

    #[test]
    fn a_multi_port_edge_fires_once_not_once_per_port() {
        // ADR-0073: one edge, one firing. An edge carrying two outputs appears
        // EXACTLY once in the fired set — the alternative (expansion into N
        // runtime edges) is precisely what the ADR rejected.
        let edges = vec![multi_edge(&["out", "spec"], "downstream", None, false)];
        assert_eq!(
            fired_targets(&edges, HashMap::new(), 1),
            vec!["downstream".to_string()]
        );
    }

    #[test]
    fn a_when_reads_the_designated_port_not_the_merged_view() {
        // The clause names WHICH carried output it reads (`spec.ready`). The
        // producer's frontmatter carries both the bare (merged) keys and the
        // port-qualified ones, so a clause on `spec` is decided by `spec`'s
        // frontmatter even when `out` declares a field of the same name with
        // the opposite value.
        let edges = vec![multi_edge(
            &["out", "spec"],
            "downstream",
            Some("spec.ready: { eq: true }"),
            false,
        )];
        let fm: HashMap<String, serde_yaml::Value> = [
            ("out.ready".to_string(), serde_yaml::Value::Bool(false)),
            ("spec.ready".to_string(), serde_yaml::Value::Bool(true)),
            // The merged view, last port wins — what an UNqualified clause reads.
            ("ready".to_string(), serde_yaml::Value::Bool(false)),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            fired_targets(&edges, fm, 1),
            vec!["downstream".to_string()],
            "the clause must read `spec`, not the merged frontmatter"
        );
    }

    #[test]
    fn a_when_on_the_other_designated_port_is_decided_by_that_port() {
        let edges = vec![multi_edge(
            &["out", "spec"],
            "downstream",
            Some("out.ready: { eq: true }"),
            false,
        )];
        let fm: HashMap<String, serde_yaml::Value> = [
            ("out.ready".to_string(), serde_yaml::Value::Bool(false)),
            ("spec.ready".to_string(), serde_yaml::Value::Bool(true)),
        ]
        .into_iter()
        .collect();
        assert!(fired_targets(&edges, fm, 1).is_empty());
    }

    #[test]
    fn a_single_port_edge_still_reads_the_unqualified_clause() {
        // Backward compatibility: a pre-#843 edge names no port and reads the
        // merged frontmatter exactly as it always has.
        let edges = vec![edge(
            "out",
            "downstream",
            Some("ready: { eq: true }"),
            false,
        )];
        let fm = [("ready".to_string(), serde_yaml::Value::Bool(true))]
            .into_iter()
            .collect();
        assert_eq!(fired_targets(&edges, fm, 1), vec!["downstream".to_string()]);
    }

    #[test]
    fn a_match_on_any_carried_port_suppresses_an_else_on_that_port() {
        // The verdict of a multi-port edge settles EVERY port it carries: one
        // edge, one firing. So a fallback leaving `spec` is suppressed by a
        // match on the `[out, spec]` edge.
        let edges = vec![
            multi_edge(
                &["out", "spec"],
                "matched",
                Some("verdict: { eq: PASS }"),
                false,
            ),
            edge("spec", "fallback", None, true),
        ];
        let fm = [("verdict".to_string(), val_s("PASS"))]
            .into_iter()
            .collect();
        assert_eq!(fired_targets(&edges, fm, 1), vec!["matched".to_string()]);
    }

    #[test]
    fn a_multi_port_else_fires_only_when_none_of_its_ports_routed() {
        let edges = vec![
            edge("out", "matched", Some("verdict: { eq: PASS }"), false),
            multi_edge(&["spec", "notes"], "fallback", None, true),
        ];
        let fm: HashMap<String, serde_yaml::Value> = [("verdict".to_string(), val_s("PASS"))]
            .into_iter()
            .collect();
        // `out` matched, but the fallback carries neither `out` nor anything
        // that routed — it is still the default path for its own outputs.
        assert_eq!(
            fired_targets(&edges, fm, 1),
            vec!["matched".to_string(), "fallback".to_string()]
        );
    }
}
