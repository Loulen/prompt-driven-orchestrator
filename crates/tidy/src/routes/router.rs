//! The router side of the route coverage: the routes a router function declares.

use super::{Route, VERBS};

/// Index of the `)` closing the `(` at `open`.
pub(super) fn closing_paren(src: &[u8], open: usize) -> usize {
    let mut depth = 0usize;
    let mut i = open;
    while i < src.len() {
        match src[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            }
            b'"' => i = skip_string(src, i),
            _ => {}
        }
        i += 1;
    }
    panic!("unbalanced parenthesis at byte {open}");
}

/// Index of the closing quote of the plain string literal opening at `start`.
pub(super) fn skip_string(src: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while src[i] != b'"' {
        if src[i] == b'\\' {
            i += 1;
        }
        i += 1;
    }
    i
}

pub(super) fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// The routes declared by the router function `marker` of `src`.
pub fn router_routes(src: &str, marker: &str) -> Vec<Route> {
    let start = src
        .find(marker)
        .unwrap_or_else(|| panic!("router function `{marker}` not found"));
    let body_open = start + src[start..].find('{').unwrap();
    let body = &src[body_open..find_block_end(src, body_open)];
    let body = strip_line_comments(body);
    // `.merge(…)` is read through [`ROUTER_SOURCES`]: the merged router is
    // listed there with its own marker.
    for shape in [".nest(", ".route_service(", ".nest_service("] {
        assert!(
            !body.contains(shape),
            "the router uses `{shape}`: teach crates/tidy/src/routes/router.rs to read it"
        );
    }
    let bytes = body.as_bytes();
    let mut routes = Vec::new();
    let mut declared = 0;
    let mut cursor = 0;
    while let Some(found) = body[cursor..].find(".route(") {
        declared += 1;
        let open = cursor + found + ".route".len();
        let close = closing_paren(bytes, open);
        let args = &body[open + 1..close];
        let quote = args.find('"').expect("a route path literal");
        let end = skip_string(args.as_bytes(), quote);
        let path = args[quote + 1..end].to_string();
        let methods = top_level_verbs(&args[end + 1..]);
        assert!(!methods.is_empty(), "no method on route {path}");
        routes.extend(methods.into_iter().map(|m| (m, path.clone())));
        cursor = close;
    }
    assert!(declared > 0, "no `.route(` in `{marker}`");
    routes
}

/// End (exclusive) of the `{ … }` block opening at `open`.
pub(super) fn find_block_end(src: &str, open: usize) -> usize {
    let bytes = src.as_bytes();
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            b'"' => i = skip_string(bytes, i),
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    panic!("unbalanced block at byte {open}");
}

fn strip_line_comments(src: &str) -> String {
    src.lines()
        .map(|line| match line.find("//") {
            Some(at) if !line[..at].contains('"') => &line[..at],
            _ => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The verbs called at depth 0 of a method-router expression:
/// `get(a).post(b)`, `axum::routing::put(c).delete(d)`, `post(e).route_layer(…)`.
fn top_level_verbs(expr: &str) -> Vec<String> {
    let bytes = expr.as_bytes();
    let mut depth = 0usize;
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b'"' => i = skip_string(bytes, i),
            _ if depth == 0 && (i == 0 || !is_ident_byte(bytes[i - 1])) => {
                for verb in VERBS {
                    if expr[i..].starts_with(verb) && bytes.get(i + verb.len()) == Some(&b'(') {
                        out.push(verb.to_uppercase());
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}
