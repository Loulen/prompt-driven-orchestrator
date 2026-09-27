//! Object and string literals: schemas, names, prefixes.

use oxc_ast::ast::{Expression, ObjectExpression, ObjectPropertyKind};

use crate::domain::pipeline::FrontmatterFieldDecl;

use super::*;

pub(super) fn object_to_frontmatter(
    obj: &ObjectExpression,
) -> HashMap<String, FrontmatterFieldDecl> {
    let mut fm = HashMap::new();
    let Some(props) = object_prop_obj(obj, "properties") else {
        return fm;
    };
    let Expression::ObjectExpression(props) = props.without_parentheses() else {
        return fm;
    };
    for p in &props.properties {
        let ObjectPropertyKind::ObjectProperty(op) = p else {
            continue;
        };
        let Some(field) = op.key.static_name() else {
            continue;
        };
        let Expression::ObjectExpression(spec) = op.value.without_parentheses() else {
            continue;
        };
        let enum_vals = object_prop_obj(spec, "enum").and_then(string_array_values);
        let decl = if let Some(vals) = enum_vals {
            FrontmatterFieldDecl {
                field_type: "enum".into(),
                allowed: Some(vals),
            }
        } else {
            let ty = object_prop_obj(spec, "type")
                .and_then(string_literal_value)
                .unwrap_or_else(|| "string".to_string());
            FrontmatterFieldDecl {
                field_type: map_json_type(&ty),
                allowed: None,
            }
        };
        fm.insert(field.to_string(), decl);
    }
    fm
}

pub(super) fn map_json_type(ty: &str) -> String {
    match ty {
        "integer" | "number" => "int",
        "boolean" => "bool",
        "array" => "list",
        "string" => "string",
        other => other,
    }
    .to_string()
}

/// Look up a property by static name on an object literal expression.
pub(super) fn object_prop<'a>(expr: &'a Expression<'a>, key: &str) -> Option<&'a Expression<'a>> {
    let Expression::ObjectExpression(obj) = expr.without_parentheses() else {
        return None;
    };
    object_prop_obj(obj, key)
}

pub(super) fn object_prop_obj<'a>(
    obj: &'a ObjectExpression<'a>,
    key: &str,
) -> Option<&'a Expression<'a>> {
    for p in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(op) = p {
            if op.key.static_name().as_deref() == Some(key) {
                return Some(&op.value);
            }
        }
    }
    None
}

pub(super) fn string_literal_value(expr: &Expression) -> Option<String> {
    match expr.without_parentheses() {
        Expression::StringLiteral(s) => Some(s.value.as_str().to_string()),
        Expression::TemplateLiteral(t) if t.is_no_substitution_template() => {
            t.quasis.first().map(|q| {
                q.value
                    .cooked
                    .map(|c| c.as_str().to_string())
                    .unwrap_or_else(|| q.value.raw.as_str().to_string())
            })
        }
        _ => None,
    }
}

pub(super) fn string_array_values(expr: &Expression) -> Option<Vec<String>> {
    let Expression::ArrayExpression(arr) = expr.without_parentheses() else {
        return None;
    };
    let mut out = Vec::new();
    for el in &arr.elements {
        if let Some(e) = el.as_expression() {
            if let Expression::StringLiteral(s) = e.without_parentheses() {
                out.push(s.value.as_str().to_string());
            }
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// The static text before the first interpolation of a label expression, if any.
/// `'debugger'` -> "debugger"; `` `implementer#${iter}` `` -> "implementer#".
pub(super) fn static_prefix_of(expr: &Expression) -> Option<String> {
    match expr.without_parentheses() {
        Expression::StringLiteral(s) => Some(s.value.as_str().to_string()),
        Expression::TemplateLiteral(t) => t.quasis.first().map(|q| {
            q.value
                .cooked
                .map(|c| c.as_str().to_string())
                .unwrap_or_else(|| q.value.raw.as_str().to_string())
        }),
        _ => None,
    }
}

/// Trim trailing separators from a label prefix for a display name.
pub(super) fn clean_name(s: &str) -> String {
    s.trim_end_matches(['#', '·', '-', ' ', ':', '_'])
        .trim()
        .to_string()
}
