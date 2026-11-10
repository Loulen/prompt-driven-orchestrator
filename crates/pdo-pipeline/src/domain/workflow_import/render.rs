//! The workflow's name and its prompts (N1/N2/N3).

use oxc_ast::ast::{BinaryOperator, Declaration, Expression, Statement};

use super::*;

// ---------------------------------------------------------------------------
// meta name
// ---------------------------------------------------------------------------

pub(super) fn extract_meta_name(stmts: &[Statement]) -> Option<String> {
    for stmt in stmts {
        let decl = match stmt {
            Statement::ExportNamedDeclaration(e) => match &e.declaration {
                Some(Declaration::VariableDeclaration(d)) => Some(d),
                _ => None,
            },
            Statement::VariableDeclaration(d) => Some(d),
            _ => None,
        };
        let Some(decl) = decl else { continue };
        for d in &decl.declarations {
            if binding_name(&d.id) != Some("meta") {
                continue;
            }
            if let Some(init) = &d.init {
                if let Expression::ObjectExpression(obj) = init.without_parentheses() {
                    if let Some(name) = object_prop_obj(obj, "name").and_then(string_literal_value)
                    {
                        if !name.trim().is_empty() {
                            return Some(name);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Render a prompt expression. Returns `(body, has_static, interpolation_refs)`
/// where `has_static` is true if any static string/quasi text was found and
/// `interpolation_refs` names the `${…}` holes (for provenance edges).
pub(super) fn render_prompt(expr: &Expression) -> (String, bool, Vec<String>) {
    let mut out = String::new();
    let mut has_static = false;
    let mut refs = Vec::new();
    render_into(expr, &mut out, &mut has_static, &mut refs, 0);
    (out, has_static, refs)
}

pub(super) fn render_into(
    expr: &Expression,
    out: &mut String,
    has_static: &mut bool,
    refs: &mut Vec<String>,
    depth: u32,
) {
    if depth > MAX_DEPTH {
        return;
    }
    match expr.without_parentheses() {
        Expression::StringLiteral(s) => {
            out.push_str(s.value.as_str());
            *has_static = true;
        }
        Expression::TemplateLiteral(t) => {
            for (i, quasi) in t.quasis.iter().enumerate() {
                let text = quasi
                    .value
                    .cooked
                    .map(|c| c.as_str().to_string())
                    .unwrap_or_else(|| quasi.value.raw.as_str().to_string());
                if !text.is_empty() {
                    *has_static = true;
                }
                out.push_str(&text);
                if let Some(e) = t.expressions.get(i) {
                    let desc = describe_expr(e);
                    out.push_str(&format!("⟨input: {desc}⟩"));
                    refs.push(desc);
                }
            }
        }
        Expression::BinaryExpression(b) if b.operator == BinaryOperator::Addition => {
            render_into(&b.left, out, has_static, refs, depth + 1);
            render_into(&b.right, out, has_static, refs, depth + 1);
        }
        Expression::ConditionalExpression(c) => {
            // `cond ? A : B` — conditionally-included fragment. Keep the
            // consequent's static text + holes; skip an empty-string alternate.
            render_into(&c.consequent, out, has_static, refs, depth + 1);
            if !is_empty_string_literal(&c.alternate) {
                render_into(&c.alternate, out, has_static, refs, depth + 1);
            }
        }
        Expression::CallExpression(call) => {
            let desc = describe_expr(&call.callee);
            out.push_str(&format!("⟨TODO: prompt built at run time by {desc}()⟩"));
        }
        other => {
            let desc = describe_expr(other);
            out.push_str(&format!("⟨input: {desc}⟩"));
            refs.push(desc);
        }
    }
}

pub(super) fn is_empty_string_literal(expr: &Expression) -> bool {
    matches!(expr.without_parentheses(), Expression::StringLiteral(s) if s.value.as_str().is_empty())
}

/// A short human description of an expression, for hole markers / provenance.
pub(super) fn describe_expr(expr: &Expression) -> String {
    match expr.without_parentheses() {
        Expression::Identifier(id) => id.name.as_str().to_string(),
        Expression::StaticMemberExpression(m) => {
            format!("{}.{}", describe_expr(&m.object), m.property.name.as_str())
        }
        Expression::ComputedMemberExpression(m) => format!("{}[…]", describe_expr(&m.object)),
        Expression::CallExpression(c) => format!("{}()", describe_expr(&c.callee)),
        Expression::ThisExpression(_) => "this".to_string(),
        _ => "expr".to_string(),
    }
}
