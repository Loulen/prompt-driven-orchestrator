//! Conditional routing on edges (ADR-0011).
//!
//! When an artifact leaves a producing node's output port, **every** outgoing
//! edge whose `when:` clause is satisfied fires — multi-match fan-out, no
//! first-match ordering. An `else` edge fires iff **no sibling edge on the same
//! source port** matched. An edge with neither `when:` nor `else` is
//! unconditional and always fires.
//!
//! Reuses the mechanical predicate evaluator in [`crate::condition`] (ADR-0002):
//! conditions reference only `iter`, the source node's frontmatter fields, and
//! pipeline variables — never an LLM-router.

use std::collections::HashMap;

use crate::condition;
use crate::pipeline::EdgeDef;

/// Returns the subset of `outgoing` edges that fire for the producing node,
/// in input order. `outgoing` must be exactly the edges whose `source.node` is
/// the producing node.
pub fn fired_edges<'a>(
    outgoing: &'a [&'a EdgeDef],
    frontmatter: &HashMap<String, serde_yaml::Value>,
    vars: &HashMap<String, serde_yaml::Value>,
    iter: i64,
) -> Vec<&'a EdgeDef> {
    let ctx = condition::EvalContext::new(iter)
        .with_fields(frontmatter.clone())
        .with_variables(vars.clone());

    // First pass: which source ports had at least one guarded (`when:`) edge match?
    let mut matched_ports: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for edge in outgoing {
        if let Some(when) = &edge.when {
            if condition::evaluate_with_iter(when, &ctx) {
                matched_ports.insert(edge.source.port.as_str());
            }
        }
    }

    // Second pass: collect the firing edges.
    let mut fired = Vec::new();
    for edge in outgoing {
        let fires = if let Some(when) = &edge.when {
            condition::evaluate_with_iter(when, &ctx)
        } else if edge.is_else {
            // `else` fires iff no sibling on the same source port matched.
            !matched_ports.contains(edge.source.port.as_str())
        } else {
            // Unconditional edge: always fires.
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
    use crate::pipeline::{EdgeDef, EdgeEndpoint};
    use pretty_assertions::assert_eq;

    fn edge(src_port: &str, tgt: &str, when: Option<&str>, is_else: bool) -> EdgeDef {
        EdgeDef {
            source: EdgeEndpoint {
                node: "producer".into(),
                port: src_port.into(),
            },
            target: EdgeEndpoint {
                node: tgt.into(),
                port: "in".into(),
            },
            reason: None,
            when: when.map(|s| serde_yaml::from_str(s).unwrap()),
            is_else,
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

        // Sibling matched → else suppressed.
        let matched = [("verdict".to_string(), val_s("FAIL"))]
            .into_iter()
            .collect();
        assert_eq!(
            fired_targets(&edges, matched, 1),
            vec!["implementer".to_string()]
        );

        // No sibling matched → else fires.
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
        // Port `a` matched; port `b` had no guarded sibling, so its else fires.
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
}
