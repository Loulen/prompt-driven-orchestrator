use std::collections::HashMap;

use crate::condition;
use crate::pipeline::{NodeDef, NodeType};

pub fn route<'a>(
    switch: &'a NodeDef,
    frontmatter: &HashMap<String, serde_yaml::Value>,
    vars: &HashMap<String, serde_yaml::Value>,
    iter: i64,
) -> &'a str {
    assert_eq!(switch.node_type, NodeType::Switch);

    let ctx = condition::EvalContext::new(iter)
        .with_fields(frontmatter.clone())
        .with_variables(vars.clone());

    for port in &switch.outputs {
        if port.name == "default" {
            continue;
        }
        if let Some(when) = &port.when {
            if condition::evaluate_with_iter(when, &ctx) {
                return &port.name;
            }
        }
    }

    "default"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{NodeDef, NodeType, Port, PortSide, PortType};
    use pretty_assertions::assert_eq;

    fn yaml(s: &str) -> serde_yaml::Value {
        serde_yaml::from_str(s).unwrap()
    }

    fn make_switch(outputs: Vec<Port>) -> NodeDef {
        NodeDef {
            id: "sw".into(),
            name: "test-switch".into(),
            node_type: NodeType::Switch,
            inputs: vec![Port {
                name: "in".into(),
                repeated: false,
                side: Some(PortSide::Left),
                port_type: PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
            }],
            outputs,
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
        }
    }

    fn port_with_when(name: &str, when_yaml: &str) -> Port {
        Port {
            name: name.into(),
            repeated: false,
            side: Some(PortSide::Right),
            port_type: PortType::Markdown,
            frontmatter: None,
            when: Some(yaml(when_yaml)),
            description: None,
        }
    }

    fn default_port() -> Port {
        Port {
            name: "default".into(),
            repeated: false,
            side: Some(PortSide::Right),
            port_type: PortType::Markdown,
            frontmatter: None,
            when: None,
            description: None,
        }
    }

    fn val_s(s: &str) -> serde_yaml::Value {
        serde_yaml::Value::String(s.into())
    }

    fn val_i(n: i64) -> serde_yaml::Value {
        serde_yaml::Value::Number(serde_yaml::Number::from(n))
    }

    // --- first-match-wins ---

    #[test]
    fn first_match_wins() {
        let sw = make_switch(vec![
            port_with_when("pass", "verdict: { eq: PASS }"),
            port_with_when("also_pass", "verdict: { eq: PASS }"),
            default_port(),
        ]);
        let fm: HashMap<String, serde_yaml::Value> =
            [("verdict".into(), val_s("PASS"))].into_iter().collect();
        assert_eq!(route(&sw, &fm, &HashMap::new(), 1), "pass");
    }

    // --- all 8 operators ---

    #[test]
    fn op_eq() {
        let sw = make_switch(vec![
            port_with_when("hit", "score: { eq: 10 }"),
            default_port(),
        ]);
        let fm = [("score".into(), val_i(10))].into_iter().collect();
        assert_eq!(route(&sw, &fm, &HashMap::new(), 1), "hit");
    }

    #[test]
    fn op_neq() {
        let sw = make_switch(vec![
            port_with_when("hit", "verdict: { neq: PASS }"),
            default_port(),
        ]);
        let fm = [("verdict".into(), val_s("FAIL"))].into_iter().collect();
        assert_eq!(route(&sw, &fm, &HashMap::new(), 1), "hit");
    }

    #[test]
    fn op_lt() {
        let sw = make_switch(vec![
            port_with_when("low", "score: { lt: 5 }"),
            default_port(),
        ]);
        let fm = [("score".into(), val_i(3))].into_iter().collect();
        assert_eq!(route(&sw, &fm, &HashMap::new(), 1), "low");
    }

    #[test]
    fn op_lte() {
        let sw = make_switch(vec![
            port_with_when("low", "score: { lte: 5 }"),
            default_port(),
        ]);
        let fm = [("score".into(), val_i(5))].into_iter().collect();
        assert_eq!(route(&sw, &fm, &HashMap::new(), 1), "low");
    }

    #[test]
    fn op_gt() {
        let sw = make_switch(vec![
            port_with_when("high", "score: { gt: 5 }"),
            default_port(),
        ]);
        let fm = [("score".into(), val_i(7))].into_iter().collect();
        assert_eq!(route(&sw, &fm, &HashMap::new(), 1), "high");
    }

    #[test]
    fn op_gte() {
        let sw = make_switch(vec![
            port_with_when("high", "score: { gte: 5 }"),
            default_port(),
        ]);
        let fm = [("score".into(), val_i(5))].into_iter().collect();
        assert_eq!(route(&sw, &fm, &HashMap::new(), 1), "high");
    }

    #[test]
    fn op_in() {
        let sw = make_switch(vec![
            port_with_when("pass", "verdict: { in: [PASS, APPROVED] }"),
            default_port(),
        ]);
        let fm = [("verdict".into(), val_s("APPROVED"))]
            .into_iter()
            .collect();
        assert_eq!(route(&sw, &fm, &HashMap::new(), 1), "pass");
    }

    #[test]
    fn op_not_in() {
        let sw = make_switch(vec![
            port_with_when("fail", "verdict: { not_in: [PASS, APPROVED] }"),
            default_port(),
        ]);
        let fm = [("verdict".into(), val_s("FAIL"))].into_iter().collect();
        assert_eq!(route(&sw, &fm, &HashMap::new(), 1), "fail");
    }

    // --- AND-of-rows ---

    #[test]
    fn and_of_rows_all_match() {
        let sw = make_switch(vec![
            port_with_when("hit", "verdict: { eq: PASS }\nscore: { gte: 8 }"),
            default_port(),
        ]);
        let fm: HashMap<String, serde_yaml::Value> = [
            ("verdict".into(), val_s("PASS")),
            ("score".into(), val_i(9)),
        ]
        .into_iter()
        .collect();
        assert_eq!(route(&sw, &fm, &HashMap::new(), 1), "hit");
    }

    #[test]
    fn and_of_rows_partial_match_falls_through() {
        let sw = make_switch(vec![
            port_with_when("hit", "verdict: { eq: PASS }\nscore: { gte: 8 }"),
            default_port(),
        ]);
        let fm: HashMap<String, serde_yaml::Value> = [
            ("verdict".into(), val_s("PASS")),
            ("score".into(), val_i(5)),
        ]
        .into_iter()
        .collect();
        assert_eq!(route(&sw, &fm, &HashMap::new(), 1), "default");
    }

    // --- $<var> resolution ---

    #[test]
    fn dollar_var_resolved() {
        let sw = make_switch(vec![
            port_with_when("hit", "score: { gte: \"$threshold\" }"),
            default_port(),
        ]);
        let fm = [("score".into(), val_i(8))].into_iter().collect();
        let vars = [("threshold".into(), val_i(7))].into_iter().collect();
        assert_eq!(route(&sw, &fm, &vars, 1), "hit");
    }

    #[test]
    fn dollar_var_no_match() {
        let sw = make_switch(vec![
            port_with_when("hit", "score: { gte: \"$threshold\" }"),
            default_port(),
        ]);
        let fm = [("score".into(), val_i(5))].into_iter().collect();
        let vars = [("threshold".into(), val_i(7))].into_iter().collect();
        assert_eq!(route(&sw, &fm, &vars, 1), "default");
    }

    // --- default fallthrough ---

    #[test]
    fn default_fallthrough_when_no_match() {
        let sw = make_switch(vec![
            port_with_when("pass", "verdict: { eq: PASS }"),
            default_port(),
        ]);
        let fm = [("verdict".into(), val_s("FAIL"))].into_iter().collect();
        assert_eq!(route(&sw, &fm, &HashMap::new(), 1), "default");
    }

    #[test]
    fn default_fallthrough_empty_frontmatter() {
        let sw = make_switch(vec![
            port_with_when("pass", "verdict: { eq: PASS }"),
            default_port(),
        ]);
        assert_eq!(route(&sw, &HashMap::new(), &HashMap::new(), 1), "default");
    }

    // --- missing frontmatter field ---

    #[test]
    fn missing_field_does_not_match() {
        let sw = make_switch(vec![
            port_with_when("pass", "verdict: { eq: PASS }"),
            default_port(),
        ]);
        let fm = [("unrelated".into(), val_s("foo"))].into_iter().collect();
        assert_eq!(route(&sw, &fm, &HashMap::new(), 1), "default");
    }

    // --- iter-based routing ---

    #[test]
    fn iter_based_routing() {
        let sw = make_switch(vec![
            port_with_when("first_run", "iter: { eq: 1 }"),
            port_with_when("retry", "iter: { gt: 1 }"),
            default_port(),
        ]);
        assert_eq!(route(&sw, &HashMap::new(), &HashMap::new(), 1), "first_run");
        assert_eq!(route(&sw, &HashMap::new(), &HashMap::new(), 3), "retry");
    }

    // --- in operator with list ---

    #[test]
    fn in_operator_with_frontmatter_list() {
        let sw = make_switch(vec![
            port_with_when("approved", "verdict: { in: [PASS, APPROVED, LGTM] }"),
            default_port(),
        ]);
        let fm = [("verdict".into(), val_s("LGTM"))].into_iter().collect();
        assert_eq!(route(&sw, &fm, &HashMap::new(), 1), "approved");
    }
}
