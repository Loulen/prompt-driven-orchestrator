use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct EvalContext {
    pub iter: i64,
    pub fields: HashMap<String, serde_yaml::Value>,
}

impl EvalContext {
    pub fn new(iter: i64) -> Self {
        Self {
            iter,
            fields: HashMap::new(),
        }
    }
}

fn evaluate_predicate(
    predicate: &serde_yaml::Value,
    field_value: Option<&serde_yaml::Value>,
) -> bool {
    let Some(pred_map) = predicate.as_mapping() else {
        return false;
    };

    for (op_key, op_value) in pred_map {
        let Some(op) = op_key.as_str() else {
            return false;
        };

        if !apply_op(op, field_value, op_value) {
            return false;
        }
    }

    true
}

fn apply_op(
    op: &str,
    field_value: Option<&serde_yaml::Value>,
    operand: &serde_yaml::Value,
) -> bool {
    match op {
        "eq" => val_eq(field_value, operand),
        "neq" => !val_eq(field_value, operand),
        "lt" => val_cmp(field_value, operand).is_some_and(|o| o.is_lt()),
        "lte" => val_cmp(field_value, operand).is_some_and(|o| o.is_le()),
        "gt" => val_cmp(field_value, operand).is_some_and(|o| o.is_gt()),
        "gte" => val_cmp(field_value, operand).is_some_and(|o| o.is_ge()),
        "in" => val_in(field_value, operand),
        "not_in" => !val_in(field_value, operand),
        _ => false,
    }
}

fn val_eq(field_value: Option<&serde_yaml::Value>, operand: &serde_yaml::Value) -> bool {
    let Some(fv) = field_value else {
        return false;
    };
    yaml_values_equal(fv, operand)
}

fn yaml_values_equal(a: &serde_yaml::Value, b: &serde_yaml::Value) -> bool {
    match (a, b) {
        (serde_yaml::Value::Number(na), serde_yaml::Value::Number(nb)) => {
            to_f64_num(na) == to_f64_num(nb)
        }
        (serde_yaml::Value::String(sa), serde_yaml::Value::String(sb)) => sa == sb,
        (serde_yaml::Value::Bool(ba), serde_yaml::Value::Bool(bb)) => ba == bb,
        (serde_yaml::Value::Null, serde_yaml::Value::Null) => true,
        _ => false,
    }
}

fn val_cmp(
    field_value: Option<&serde_yaml::Value>,
    operand: &serde_yaml::Value,
) -> Option<std::cmp::Ordering> {
    let fv = field_value?;
    match (fv, operand) {
        (serde_yaml::Value::Number(na), serde_yaml::Value::Number(nb)) => {
            to_f64_num(na).partial_cmp(&to_f64_num(nb))
        }
        (serde_yaml::Value::String(sa), serde_yaml::Value::String(sb)) => Some(sa.cmp(sb)),
        _ => None,
    }
}

fn val_in(field_value: Option<&serde_yaml::Value>, operand: &serde_yaml::Value) -> bool {
    let Some(fv) = field_value else {
        return false;
    };
    let Some(seq) = operand.as_sequence() else {
        return false;
    };
    seq.iter().any(|item| yaml_values_equal(fv, item))
}

fn to_f64_num(n: &serde_yaml::Number) -> f64 {
    if let Some(i) = n.as_i64() {
        i as f64
    } else if let Some(u) = n.as_u64() {
        u as f64
    } else {
        n.as_f64().unwrap_or(f64::NAN)
    }
}

pub fn evaluate_with_iter(when: &serde_yaml::Value, ctx: &EvalContext) -> bool {
    let Some(map) = when.as_mapping() else {
        return false;
    };

    for (key, predicate) in map {
        let Some(field_name) = key.as_str() else {
            return false;
        };

        if field_name == "any" {
            if !evaluate_any_with_iter(predicate, ctx) {
                return false;
            }
            continue;
        }

        if field_name == "iter" {
            let iter_val = serde_yaml::Value::Number(serde_yaml::Number::from(ctx.iter));
            if !evaluate_predicate(predicate, Some(&iter_val)) {
                return false;
            }
            continue;
        }

        let field_value = ctx.fields.get(field_name);
        if !evaluate_predicate(predicate, field_value) {
            return false;
        }
    }

    true
}

fn evaluate_any_with_iter(clauses: &serde_yaml::Value, ctx: &EvalContext) -> bool {
    let Some(seq) = clauses.as_sequence() else {
        return false;
    };

    for clause in seq {
        if evaluate_with_iter(clause, ctx) {
            return true;
        }
    }

    false
}

pub fn render_halt_message(template: &str, ctx: &HaltContext) -> String {
    template
        .replace("{iter}", &ctx.iter.to_string())
        .replace("{node-id}", &ctx.node_id)
}

pub struct HaltContext {
    pub iter: i64,
    pub node_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn yaml(s: &str) -> serde_yaml::Value {
        serde_yaml::from_str(s).unwrap()
    }

    fn ctx_iter(iter: i64) -> EvalContext {
        EvalContext::new(iter)
    }

    // --- eq ---

    #[test]
    fn eq_integer_match() {
        let when = yaml("iter: { eq: 3 }");
        assert!(evaluate_with_iter(&when, &ctx_iter(3)));
    }

    #[test]
    fn eq_integer_no_match() {
        let when = yaml("iter: { eq: 3 }");
        assert!(!evaluate_with_iter(&when, &ctx_iter(5)));
    }

    #[test]
    fn eq_string_field() {
        let when = yaml("verdict: { eq: PASS }");
        let mut ctx = ctx_iter(1);
        ctx.fields
            .insert("verdict".into(), serde_yaml::Value::String("PASS".into()));
        assert!(evaluate_with_iter(&when, &ctx));
    }

    // --- neq ---

    #[test]
    fn neq_integer_match() {
        let when = yaml("iter: { neq: 3 }");
        assert!(evaluate_with_iter(&when, &ctx_iter(5)));
    }

    #[test]
    fn neq_integer_no_match() {
        let when = yaml("iter: { neq: 3 }");
        assert!(!evaluate_with_iter(&when, &ctx_iter(3)));
    }

    // --- lt ---

    #[test]
    fn lt_true() {
        let when = yaml("iter: { lt: 5 }");
        assert!(evaluate_with_iter(&when, &ctx_iter(3)));
    }

    #[test]
    fn lt_false_equal() {
        let when = yaml("iter: { lt: 5 }");
        assert!(!evaluate_with_iter(&when, &ctx_iter(5)));
    }

    #[test]
    fn lt_false_greater() {
        let when = yaml("iter: { lt: 5 }");
        assert!(!evaluate_with_iter(&when, &ctx_iter(7)));
    }

    // --- lte ---

    #[test]
    fn lte_true_less() {
        let when = yaml("iter: { lte: 5 }");
        assert!(evaluate_with_iter(&when, &ctx_iter(3)));
    }

    #[test]
    fn lte_true_equal() {
        let when = yaml("iter: { lte: 5 }");
        assert!(evaluate_with_iter(&when, &ctx_iter(5)));
    }

    #[test]
    fn lte_false() {
        let when = yaml("iter: { lte: 5 }");
        assert!(!evaluate_with_iter(&when, &ctx_iter(6)));
    }

    // --- gt ---

    #[test]
    fn gt_true() {
        let when = yaml("iter: { gt: 3 }");
        assert!(evaluate_with_iter(&when, &ctx_iter(5)));
    }

    #[test]
    fn gt_false_equal() {
        let when = yaml("iter: { gt: 3 }");
        assert!(!evaluate_with_iter(&when, &ctx_iter(3)));
    }

    #[test]
    fn gt_false_less() {
        let when = yaml("iter: { gt: 3 }");
        assert!(!evaluate_with_iter(&when, &ctx_iter(1)));
    }

    // --- gte ---

    #[test]
    fn gte_true_greater() {
        let when = yaml("iter: { gte: 3 }");
        assert!(evaluate_with_iter(&when, &ctx_iter(5)));
    }

    #[test]
    fn gte_true_equal() {
        let when = yaml("iter: { gte: 3 }");
        assert!(evaluate_with_iter(&when, &ctx_iter(3)));
    }

    #[test]
    fn gte_false() {
        let when = yaml("iter: { gte: 3 }");
        assert!(!evaluate_with_iter(&when, &ctx_iter(2)));
    }

    // --- in ---

    #[test]
    fn in_string_match() {
        let when = yaml("verdict: { in: [PASS, APPROVED] }");
        let mut ctx = ctx_iter(1);
        ctx.fields
            .insert("verdict".into(), serde_yaml::Value::String("PASS".into()));
        assert!(evaluate_with_iter(&when, &ctx));
    }

    #[test]
    fn in_string_no_match() {
        let when = yaml("verdict: { in: [PASS, APPROVED] }");
        let mut ctx = ctx_iter(1);
        ctx.fields
            .insert("verdict".into(), serde_yaml::Value::String("FAIL".into()));
        assert!(!evaluate_with_iter(&when, &ctx));
    }

    #[test]
    fn in_integer_match() {
        let when = yaml("iter: { in: [1, 2, 3] }");
        assert!(evaluate_with_iter(&when, &ctx_iter(2)));
    }

    // --- not_in ---

    #[test]
    fn not_in_match() {
        let when = yaml("verdict: { not_in: [PASS, APPROVED] }");
        let mut ctx = ctx_iter(1);
        ctx.fields
            .insert("verdict".into(), serde_yaml::Value::String("FAIL".into()));
        assert!(evaluate_with_iter(&when, &ctx));
    }

    #[test]
    fn not_in_no_match() {
        let when = yaml("verdict: { not_in: [PASS, APPROVED] }");
        let mut ctx = ctx_iter(1);
        ctx.fields
            .insert("verdict".into(), serde_yaml::Value::String("PASS".into()));
        assert!(!evaluate_with_iter(&when, &ctx));
    }

    // --- implicit AND ---

    #[test]
    fn implicit_and_all_true() {
        let when = yaml("iter: { lt: 5 }\nverdict: { neq: PASS }");
        let mut ctx = ctx_iter(2);
        ctx.fields
            .insert("verdict".into(), serde_yaml::Value::String("FAIL".into()));
        assert!(evaluate_with_iter(&when, &ctx));
    }

    #[test]
    fn implicit_and_one_false() {
        let when = yaml("iter: { lt: 5 }\nverdict: { eq: PASS }");
        let mut ctx = ctx_iter(2);
        ctx.fields
            .insert("verdict".into(), serde_yaml::Value::String("FAIL".into()));
        assert!(!evaluate_with_iter(&when, &ctx));
    }

    // --- any: (OR) ---

    #[test]
    fn any_or_first_clause_true() {
        let when = yaml("any:\n  - iter: { eq: 1 }\n  - iter: { eq: 5 }");
        assert!(evaluate_with_iter(&when, &ctx_iter(1)));
    }

    #[test]
    fn any_or_second_clause_true() {
        let when = yaml("any:\n  - iter: { eq: 1 }\n  - iter: { eq: 5 }");
        assert!(evaluate_with_iter(&when, &ctx_iter(5)));
    }

    #[test]
    fn any_or_none_true() {
        let when = yaml("any:\n  - iter: { eq: 1 }\n  - iter: { eq: 5 }");
        assert!(!evaluate_with_iter(&when, &ctx_iter(3)));
    }

    // --- edge cases ---

    #[test]
    fn missing_field_returns_false() {
        let when = yaml("verdict: { eq: PASS }");
        let ctx = ctx_iter(1);
        assert!(!evaluate_with_iter(&when, &ctx));
    }

    #[test]
    fn type_mismatch_returns_false() {
        let when = yaml("verdict: { lt: 5 }");
        let mut ctx = ctx_iter(1);
        ctx.fields
            .insert("verdict".into(), serde_yaml::Value::String("FAIL".into()));
        assert!(!evaluate_with_iter(&when, &ctx));
    }

    #[test]
    fn empty_when_clause_is_true() {
        let when = yaml("{}");
        assert!(evaluate_with_iter(&when, &ctx_iter(1)));
    }

    #[test]
    fn non_mapping_when_is_false() {
        let when = yaml("42");
        assert!(!evaluate_with_iter(&when, &ctx_iter(1)));
    }

    #[test]
    fn unknown_operator_returns_false() {
        let when = yaml("iter: { contains: 3 }");
        assert!(!evaluate_with_iter(&when, &ctx_iter(3)));
    }

    #[test]
    fn in_with_non_sequence_returns_false() {
        let when = yaml("verdict: { in: PASS }");
        let mut ctx = ctx_iter(1);
        ctx.fields
            .insert("verdict".into(), serde_yaml::Value::String("PASS".into()));
        assert!(!evaluate_with_iter(&when, &ctx));
    }

    // --- halt message rendering ---

    #[test]
    fn render_halt_message_basic() {
        let msg = render_halt_message(
            "Blocked after {iter} iterations on {node-id}",
            &HaltContext {
                iter: 5,
                node_id: "reviewer".into(),
            },
        );
        assert_eq!(msg, "Blocked after 5 iterations on reviewer");
    }

    #[test]
    fn render_halt_message_no_placeholders() {
        let msg = render_halt_message(
            "Pipeline halted",
            &HaltContext {
                iter: 1,
                node_id: "x".into(),
            },
        );
        assert_eq!(msg, "Pipeline halted");
    }

    // --- combined predicate on single field ---

    #[test]
    fn combined_predicates_on_iter() {
        let when = yaml("iter: { gte: 2, lte: 4 }");
        assert!(!evaluate_with_iter(&when, &ctx_iter(1)));
        assert!(evaluate_with_iter(&when, &ctx_iter(2)));
        assert!(evaluate_with_iter(&when, &ctx_iter(3)));
        assert!(evaluate_with_iter(&when, &ctx_iter(4)));
        assert!(!evaluate_with_iter(&when, &ctx_iter(5)));
    }
}
