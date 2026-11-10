//! `if (x OP v)` guards become `when:` clauses; loop bounds.

use oxc_ast::ast::{BinaryOperator, Expression, LogicalOperator};

use super::*;

pub(super) struct Predicate {
    pub(super) object: Option<String>,
    pub(super) field: String,
    pub(super) op: String,
    pub(super) value: serde_yaml::Value,
}

impl Predicate {
    pub(super) fn when_value(&self) -> serde_yaml::Value {
        let mut inner = serde_yaml::Mapping::new();
        inner.insert(
            serde_yaml::Value::String(self.op.clone()),
            self.value.clone(),
        );
        let mut outer = serde_yaml::Mapping::new();
        outer.insert(
            serde_yaml::Value::String(self.field.clone()),
            serde_yaml::Value::Mapping(inner),
        );
        serde_yaml::Value::Mapping(outer)
    }
}

pub(super) fn extract_predicate(expr: &Expression) -> Option<Predicate> {
    match expr.without_parentheses() {
        Expression::BinaryExpression(b) => {
            let op = map_binop(b.operator)?;
            if let Some(value) = literal_to_yaml(&b.right) {
                let (field, object) = field_of(&b.left)?;
                Some(Predicate {
                    object,
                    field,
                    op: op.to_string(),
                    value,
                })
            } else if let Some(value) = literal_to_yaml(&b.left) {
                let (field, object) = field_of(&b.right)?;
                Some(Predicate {
                    object,
                    field,
                    op: flip_op(op).to_string(),
                    value,
                })
            } else {
                None
            }
        }
        Expression::LogicalExpression(l) => {
            extract_predicate(&l.left).or_else(|| extract_predicate(&l.right))
        }
        _ => None,
    }
}

/// `x` -> ("x", None); `a.b` -> ("b", Some("a")).
pub(super) fn field_of(expr: &Expression) -> Option<(String, Option<String>)> {
    match expr.without_parentheses() {
        Expression::Identifier(id) => Some((id.name.as_str().to_string(), None)),
        Expression::StaticMemberExpression(m) => {
            let object = match m.object.without_parentheses() {
                Expression::Identifier(id) => Some(id.name.as_str().to_string()),
                _ => None,
            };
            Some((m.property.name.as_str().to_string(), object))
        }
        _ => None,
    }
}

pub(super) fn map_binop(op: BinaryOperator) -> Option<&'static str> {
    match op {
        BinaryOperator::Equality | BinaryOperator::StrictEquality => Some("eq"),
        BinaryOperator::Inequality | BinaryOperator::StrictInequality => Some("neq"),
        BinaryOperator::LessThan => Some("lt"),
        BinaryOperator::LessEqualThan => Some("lte"),
        BinaryOperator::GreaterThan => Some("gt"),
        BinaryOperator::GreaterEqualThan => Some("gte"),
        _ => None,
    }
}

pub(super) fn flip_op(op: &str) -> &str {
    match op {
        "lt" => "gt",
        "lte" => "gte",
        "gt" => "lt",
        "gte" => "lte",
        other => other,
    }
}

pub(super) fn literal_to_yaml(expr: &Expression) -> Option<serde_yaml::Value> {
    match expr.without_parentheses() {
        Expression::StringLiteral(s) => {
            Some(serde_yaml::Value::String(s.value.as_str().to_string()))
        }
        Expression::NumericLiteral(n) => {
            if n.value.fract() == 0.0 && n.value.abs() < i64::MAX as f64 {
                Some(serde_yaml::Value::Number(serde_yaml::Number::from(
                    n.value as i64,
                )))
            } else {
                Some(serde_yaml::Value::Number(serde_yaml::Number::from(n.value)))
            }
        }
        Expression::BooleanLiteral(b) => Some(serde_yaml::Value::Bool(b.value)),
        _ => None,
    }
}

pub(super) enum MaxIter {
    Value(i64),
    Unresolved,
}

pub(super) fn resolve_for_bound(test: Option<&Expression>, consts: &Consts) -> MaxIter {
    resolve_test_bound(test, consts)
}

pub(super) fn resolve_test_bound(test: Option<&Expression>, consts: &Consts) -> MaxIter {
    let Some(t) = test else {
        return MaxIter::Unresolved;
    };
    if let Expression::BinaryExpression(b) = t.without_parentheses() {
        if let Some(n) = resolve_number(&b.right, consts) {
            return MaxIter::Value(n as i64);
        }
        if let Some(n) = resolve_number(&b.left, consts) {
            return MaxIter::Value(n as i64);
        }
    }
    if let Expression::LogicalExpression(l) = t.without_parentheses() {
        // `while (cond && iter < N)` — probe both sides.
        if let MaxIter::Value(v) = resolve_test_bound(Some(&l.left), consts) {
            return MaxIter::Value(v);
        }
        if let MaxIter::Value(v) = resolve_test_bound(Some(&l.right), consts) {
            return MaxIter::Value(v);
        }
    }
    MaxIter::Unresolved
}

pub(super) fn resolve_number(expr: &Expression, consts: &Consts) -> Option<f64> {
    match expr.without_parentheses() {
        Expression::NumericLiteral(n) => Some(n.value),
        Expression::Identifier(id) => consts.numbers.get(id.name.as_str()).copied(),
        Expression::LogicalExpression(l) if l.operator == LogicalOperator::Or => {
            resolve_number(&l.left, consts).or_else(|| resolve_number(&l.right, consts))
        }
        Expression::ConditionalExpression(c) => {
            resolve_number(&c.consequent, consts).or_else(|| resolve_number(&c.alternate, consts))
        }
        Expression::UnaryExpression(u)
            if u.operator == oxc_ast::ast::UnaryOperator::UnaryNegation =>
        {
            resolve_number(&u.argument, consts).map(|n| -n)
        }
        _ => None,
    }
}
