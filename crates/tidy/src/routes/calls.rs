//! The test side of the route coverage: the routes a test source calls, and with
//! which method.

use std::collections::{BTreeMap, BTreeSet};

use super::router::{closing_paren, find_block_end, is_ident_byte, skip_string};
use super::{Route, VERBS};

/// Every string literal of `src` with its byte offset — comments, char literals
/// and lifetimes skipped, raw strings read whole.
fn string_literals(src: &str) -> Vec<(usize, String)> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let prev_ident = i > 0 && is_ident_byte(b[i - 1]);
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i = src[i + 2..].find("*/").map_or(b.len(), |e| i + 2 + e + 2);
                continue;
            }
            b'r' if !prev_ident && matches!(b.get(i + 1), Some(b'"') | Some(b'#')) => {
                let hashes = b[i + 1..].iter().take_while(|c| **c == b'#').count();
                if b.get(i + 1 + hashes) == Some(&b'"') {
                    let body = i + 2 + hashes;
                    let closing = format!("\"{}", "#".repeat(hashes));
                    let end = body + src[body..].find(&closing).expect("raw string ends");
                    out.push((i, src[body..end].to_string()));
                    i = end + closing.len();
                    continue;
                }
            }
            b'"' => {
                let end = skip_string(b, i);
                out.push((i, src[i + 1..end].to_string()));
                i = end + 1;
                continue;
            }
            b'\'' => {
                // A char literal (`'"'`, `'\n'`) — or a lifetime, which has no
                // closing quote within three bytes.
                if b.get(i + 1) == Some(&b'\\') {
                    i += src[i + 2..].find('\'').map_or(2, |e| e + 3);
                    continue;
                }
                if b.get(i + 2) == Some(&b'\'') {
                    i += 3;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// The path segments a URL literal names: an optional `{…}` or `scheme://host`
/// prefix, then an absolute path; query and fragment dropped.
fn literal_segments(literal: &str) -> Option<Vec<String>> {
    let rest = if let Some(after) = literal.strip_prefix('{') {
        &after[after.find('}')? + 1..]
    } else if let Some(at) = literal.find("://") {
        let after = &literal[at + 3..];
        &after[after.find('/')?..]
    } else {
        literal
    };
    let path = rest.strip_prefix('/')?;
    let path = path.split(['?', '#']).next().unwrap_or("");
    if path.contains(char::is_whitespace) {
        return None;
    }
    Some(path.split('/').map(str::to_string).collect())
}

fn template_matches(template: &str, segments: &[String]) -> bool {
    let parts: Vec<&str> = template[1..].split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        if part.starts_with("{*") {
            return segments.len() > i;
        }
        let Some(segment) = segments.get(i) else {
            return false;
        };
        if part.starts_with('{') {
            if segment.is_empty() {
                return false;
            }
        } else if part != segment {
            return false;
        }
    }
    segments.len() == parts.len()
}

/// Static beats a parameter beats a wildcard, segment by segment — axum's rule.
fn specificity(template: &str) -> Vec<u8> {
    template[1..]
        .split('/')
        .map(|p| {
            if p.starts_with("{*") {
                0
            } else if p.starts_with('{') {
                1
            } else {
                2
            }
        })
        .collect()
}

fn best_template<'a>(paths: &'a BTreeSet<String>, segments: &[String]) -> Option<&'a str> {
    paths
        .iter()
        .filter(|p| template_matches(p, segments))
        .max_by_key(|p| specificity(p))
        .map(String::as_str)
}

/// The HTTP verb a callee name implies, or the one verb the body of a helper of
/// the same file issues (`depth` bounds helper-of-helper lookups).
fn verb_of_callee(name: &str, src: &str, depth: usize) -> Option<String> {
    let lower = name.to_lowercase();
    if lower == "connect_async" {
        return Some("GET".into());
    }
    for verb in VERBS {
        if lower == verb
            || lower.starts_with(&format!("{verb}_"))
            || lower.ends_with(&format!("_{verb}"))
        {
            return Some(verb.to_uppercase());
        }
    }
    if depth >= 2 {
        return None;
    }
    let verbs = verbs_issued_by(fn_body(src, name)?, name, src, depth);
    (verbs.len() == 1).then(|| verbs.into_iter().next().unwrap())
}

/// The verbs a helper body issues: `.post(…)`, `reqwest::get(…)`, a
/// `Method::PUT`, or the verb of a helper it calls in turn.
fn verbs_issued_by(body: &str, name: &str, src: &str, depth: usize) -> BTreeSet<String> {
    let mut verbs = BTreeSet::new();
    for verb in VERBS {
        for sep in [".", "::"] {
            let call = format!("{sep}{verb}(");
            for (at, _) in body.match_indices(&call) {
                // `.get("key")` reads a map, not a URL.
                let arg = body[at + call.len()..].trim_start();
                if !arg.starts_with('"') {
                    verbs.insert(verb.to_uppercase());
                }
            }
        }
    }
    if body.contains("connect_async(") {
        verbs.insert("GET".into());
    }
    verbs.extend(explicit_method(body));
    if verbs.is_empty() {
        let bytes = body.as_bytes();
        for (i, _) in body.match_indices('(') {
            let mut start = i;
            while start > 0 && is_ident_byte(bytes[start - 1]) {
                start -= 1;
            }
            let callee = &body[start..i];
            if callee.is_empty() || callee == name {
                continue;
            }
            if let Some(verb) = verb_of_callee(callee, src, depth + 1) {
                verbs.insert(verb);
            }
        }
    }
    verbs
}

/// `Method::PUT` (or `reqwest::Method::PUT`) inside a call's arguments.
fn explicit_method(args: &str) -> Option<String> {
    VERBS
        .iter()
        .map(|v| v.to_uppercase())
        .find(|v| args.contains(&format!("Method::{v}")))
}

/// The body of `fn <name>` in `src`, if the file defines one.
fn fn_body<'a>(src: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("fn {name}");
    let mut from = 0;
    while let Some(at) = src[from..].find(&needle) {
        let after = from + at + needle.len();
        let boundary = src[after..]
            .chars()
            .next()
            .is_some_and(|c| c == '(' || c == '<');
        if boundary {
            let open = after + src[after..].find('{')?;
            return Some(&src[open..find_block_end(src, open)]);
        }
        from = after;
    }
    None
}

/// The call whose unmatched `(` encloses `pos` — its `(` offset and name —
/// walking outward through macro calls (`format!(`), `Some(` and `Ok(`.
/// `Err(offset)` when the walk leaves the expression first (a `;`, or an
/// unmatched `{`/`[`), with the offset just past that boundary.
fn call_around(src: &str, pos: usize) -> Result<(usize, String), usize> {
    let b = src.as_bytes();
    let mut depth = 0usize;
    let mut i = pos;
    while i > 0 {
        i -= 1;
        match b[i] {
            b')' | b']' | b'}' => depth += 1,
            b'(' | b'[' | b'{' if depth > 0 => depth -= 1,
            b'(' => {
                let mut end = i;
                while end > 0 && b[end - 1] == b' ' {
                    end -= 1;
                }
                let is_macro = end > 0 && b[end - 1] == b'!';
                if is_macro {
                    end -= 1;
                }
                let mut start = end;
                while start > 0 && is_ident_byte(b[start - 1]) {
                    start -= 1;
                }
                let name = &src[start..end];
                if is_macro || name.is_empty() || name == "Some" || name == "Ok" {
                    continue;
                }
                return Ok((i, name.to_string()));
            }
            b'[' | b'{' | b';' => return Err(i + 1),
            _ => {}
        }
    }
    Err(0)
}

/// The HTTP method of the request a URL literal at `pos` ends up in.
fn method_at(src: &str, pos: usize, literal: &str) -> Option<String> {
    if literal.starts_with("ws://") {
        return Some("GET".into());
    }
    let stmt_start = match call_around(src, pos) {
        Ok((open, name)) => return method_of_call(src, open, &name),
        Err(stmt_start) => stmt_start,
    };
    // `let url = format!(…);` — or `let url = if … { format!(…) } else …;`:
    // follow `url` to the call it is handed to.
    let var = let_binding(&src[stmt_start..pos]).or_else(|| {
        src[..stmt_start]
            .ends_with('{')
            .then(|| let_binding(statement_before(src, stmt_start - 1)))
            .flatten()
    })?;
    let tail_start = pos + literal.len();
    let tail_end = (tail_start + 3000).min(src.len());
    let bytes = src.as_bytes();
    for (at, _) in src[tail_start..tail_end].match_indices(var.as_str()) {
        let abs = tail_start + at;
        let bounded = !is_ident_byte(bytes[abs - 1])
            && !bytes
                .get(abs + var.len())
                .copied()
                .is_some_and(is_ident_byte);
        if !bounded {
            continue;
        }
        if let Ok((open, name)) = call_around(src, abs) {
            if let Some(verb) = method_of_call(src, open, &name) {
                return Some(verb);
            }
        }
    }
    None
}

fn method_of_call(src: &str, open: usize, name: &str) -> Option<String> {
    let close = closing_paren(src.as_bytes(), open);
    explicit_method(&src[open..=close]).or_else(|| verb_of_callee(name, src, 0))
}

/// The statement ending at `end`: back to the previous `;`, `{` or `}`.
fn statement_before(src: &str, end: usize) -> &str {
    let start = src[..end].rfind([';', '{', '}']).map_or(0, |s| s + 1);
    &src[start..end]
}

/// `let [mut] <var>` at the head of a statement.
fn let_binding(stmt: &str) -> Option<String> {
    let rest = stmt.trim_start().strip_prefix("let ")?.trim_start();
    let rest = rest.strip_prefix("mut ").unwrap_or(rest);
    let var: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    (!var.is_empty()).then_some(var)
}

/// `(method, template)` → the test files calling it.
pub fn calls_in(
    sources: &[(String, String)],
    paths: &BTreeSet<String>,
) -> BTreeMap<Route, BTreeSet<String>> {
    let mut covered: BTreeMap<Route, BTreeSet<String>> = BTreeMap::new();
    for (file, src) in sources {
        for (pos, literal) in string_literals(src) {
            let Some(segments) = literal_segments(&literal) else {
                continue;
            };
            let Some(template) = best_template(paths, &segments) else {
                continue;
            };
            if let Some(method) = method_at(src, pos, &literal) {
                covered
                    .entry((method, template.to_string()))
                    .or_default()
                    .insert(file.clone());
            }
        }
    }
    covered
}
