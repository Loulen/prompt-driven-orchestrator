//! Structure test (ADR-0080, #914) — **every route of the router is called by at
//! least one integration test**, method by method.
//!
//! Adding a route (or a method on an existing path) without a test against the
//! real daemon turns this red. It is one of the three structure tests of the
//! spec #913 (architecture, 400 lines, route coverage).
//!
//! How it reads both sides, statically — nextest runs one process per test, so no
//! runtime recorder could see the calls the other tests make:
//!
//! - **the routes**: the `.route("<path>", <method router>)` calls of
//!   `build_router` in the legacy crate and of every context router it merges,
//!   each top-level `get(`/`post(`/`put(`/
//!   `patch(`/`delete(` of the method router being one route. When the
//!   migration moves the router, point [`ROUTER_SOURCES`] at its new home.
//! - **the calls**: every string literal of `tests/**/*.rs` that reads as a URL
//!   path (`"/runs"`, `"{}/runs/{run_id}/refs"`, `"ws://{}/ws"`, …) is matched to
//!   the most specific route template, and its HTTP method is read from the call
//!   the literal is an argument of: `.post(…)`, `reqwest::get(…)`,
//!   `get_json(…)`, a `Method::PUT` argument, a local helper whose body issues a
//!   single method, or — for `let url = format!(…)` — the call `url` is later
//!   passed to. A `ws://` literal is a GET (the upgrade).
//!
//! A literal the reader cannot tie to a method counts for nothing: write the call
//! so it can (`client.post(format!(…))`), don't teach this file your helper.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The files holding the assembled router (relative to this crate), and the
/// function that builds each part: the legacy crate's router, and the router of
/// every context already extracted (merged by the legacy router or by the
/// composition root).
const ROUTER_SOURCES: &[(&str, &str)] = &[
    (LEGACY_ROUTER, "fn build_router("),
    (
        "../pdo-workspace/src/adapters/http/mod.rs",
        "pub fn router<S: ProjectSettings>(",
    ),
    ("../pdo-platform/src/adapters/http/mod.rs", "pub fn router("),
];

/// The legacy crate's router, which the migration empties context by context.
const LEGACY_ROUTER: &str = "../pdo-daemon/src/lib.rs";

const VERBS: [&str; 5] = ["get", "post", "put", "patch", "delete"];

/// `(METHOD, path template)`.
pub(crate) type Route = (String, String);

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

// --- The router side ---------------------------------------------------------------

/// Index of the `)` closing the `(` at `open`.
fn closing_paren(src: &[u8], open: usize) -> usize {
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
fn skip_string(src: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while src[i] != b'"' {
        if src[i] == b'\\' {
            i += 1;
        }
        i += 1;
    }
    i
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// The routes declared by the router function `marker` of `src`.
fn router_routes(src: &str, marker: &str) -> Vec<Route> {
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
            "the router uses `{shape}`: teach route_coverage.rs to read it"
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
fn find_block_end(src: &str, open: usize) -> usize {
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

// --- The test side -------------------------------------------------------------------

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
fn calls_in(
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

fn test_sources() -> Vec<(String, String)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    let tests = crate_dir().join("tests");
    let mut files = Vec::new();
    walk(&tests, &mut files);
    files.sort();
    files
        .into_iter()
        // This file names routes in its own fixtures; it calls none of them.
        .filter(|p| p.file_name().is_some_and(|n| n != "route_coverage.rs"))
        .map(|p| {
            let rel = p.strip_prefix(&tests).unwrap().display().to_string();
            (rel, std::fs::read_to_string(&p).unwrap())
        })
        .collect()
}

fn assembled_routes() -> Vec<Route> {
    ROUTER_SOURCES
        .iter()
        .flat_map(|(file, marker)| {
            let src = std::fs::read_to_string(crate_dir().join(file)).unwrap();
            router_routes(&src, marker)
        })
        .collect()
}

/// The routes of every bounded context already extracted from the legacy crate
/// — what [`ROUTER_SOURCES`] lists beside the legacy router.
pub(crate) fn context_routes() -> Vec<Route> {
    ROUTER_SOURCES
        .iter()
        .filter(|(file, _)| *file != LEGACY_ROUTER)
        .flat_map(|(file, marker)| {
            let src = std::fs::read_to_string(crate_dir().join(file)).unwrap();
            router_routes(&src, marker)
        })
        .collect()
}

fn uncovered(routes: &[Route], sources: &[(String, String)]) -> Vec<Route> {
    let paths: BTreeSet<String> = routes.iter().map(|(_, p)| p.clone()).collect();
    let covered = calls_in(sources, &paths);
    let mut missing: Vec<Route> = routes
        .iter()
        .filter(|r| !covered.contains_key(*r))
        .cloned()
        .collect();
    missing.sort();
    missing.dedup();
    missing
}

#[test]
fn every_route_is_called_by_at_least_one_integration_test() {
    let routes = assembled_routes();
    let missing = uncovered(&routes, &test_sources());
    assert!(
        missing.is_empty(),
        "{} route(s) of the router are called by no integration test (tests/**/*.rs) — \
         add a test against the real daemon (TestDaemon + reqwest) for each:\n{}",
        missing.len(),
        missing
            .iter()
            .map(|(m, p)| format!("  {m} {p}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn the_router_reader_sees_every_route_declaration() {
    let routes = assembled_routes();
    let paths: BTreeSet<&str> = routes.iter().map(|(_, p)| p.as_str()).collect();
    // Spot checks on the shapes the reader must handle.
    for expected in [
        ("GET", "/ws"),
        ("PUT", "/pipelines/{pipeline_id}"),
        ("DELETE", "/pipelines/{pipeline_id}"),
        ("POST", "/runs"),
        ("PATCH", "/runs/{run_id}/repos"),
        ("GET", "/pages/{name}/{*path}"),
        ("DELETE", "/stats/absorptions/member"),
    ] {
        assert!(
            routes.contains(&(expected.0.to_string(), expected.1.to_string())),
            "{expected:?} missing from {routes:?}"
        );
    }
    assert!(paths.len() > 100, "{} paths only", paths.len());
}

/// The demonstration the issue asks for, kept as a test: a fictitious route, and
/// a fictitious method on a covered path, both come out uncovered.
#[test]
fn a_route_without_a_test_turns_the_coverage_red() {
    let src = std::fs::read_to_string(crate_dir().join("../pdo-daemon/src/lib.rs")).unwrap();
    let injected = src.replacen(
        "fn build_router(state: Arc<AppState>) -> Router {\n    Router::new()\n",
        "fn build_router(state: Arc<AppState>) -> Router {\n    Router::new()\n        \
         .route(\"/net-914/fictitious\", get(fictitious))\n        \
         .route(\"/runs/reapable\", axum::routing::delete(fictitious))\n",
        1,
    );
    assert_ne!(injected, src, "the injection point moved");
    let routes = router_routes(&injected, "fn build_router(");
    assert_eq!(
        uncovered(&routes, &test_sources()),
        vec![
            ("DELETE".to_string(), "/runs/reapable".to_string()),
            ("GET".to_string(), "/net-914/fictitious".to_string()),
        ]
    );
}

#[test]
fn the_call_reader_ties_literals_to_their_method() {
    let routes: Vec<Route> = [
        ("GET", "/runs"),
        ("POST", "/runs"),
        ("GET", "/runs/{run_id}"),
        ("GET", "/runs/reapable"),
        ("DELETE", "/runs/{run_id}"),
        ("PUT", "/settings"),
        ("POST", "/runs/{run_id}/nodes/{node_id}/skip"),
        ("GET", "/ws"),
    ]
    .iter()
    .map(|(m, p)| (m.to_string(), p.to_string()))
    .collect();
    let paths: BTreeSet<String> = routes.iter().map(|(_, p)| p.clone()).collect();
    let src = r#####"
async fn raw_post(d: &D, path: &str) {}
async fn transition(d: &D, path: &str) { raw_post(d, path).await; }
async fn t(daemon: &D) {
    client.post(format!("{}/runs", daemon.url())).send();
    let x = daemon.get_json(&format!("/runs/{run_id}")).await;
    let url = format!("{}/runs/{}", daemon.url(), id);
    client.delete(&url).send();
    send(&daemon, reqwest::Method::PUT, "/settings", None).await;
    transition(&daemon, &format!("/runs/{run_id}/nodes/a/skip")).await;
    assert!(text.contains("/runs/reapable"));
    let ws = format!("ws://{}/ws", daemon.addr);
}
"#####;
    let covered = calls_in(&[("t.rs".to_string(), src.to_string())], &paths);
    let got: BTreeSet<&Route> = covered.keys().collect();
    let want: BTreeSet<Route> = [
        ("POST", "/runs"),
        ("GET", "/runs/{run_id}"),
        ("DELETE", "/runs/{run_id}"),
        ("PUT", "/settings"),
        ("POST", "/runs/{run_id}/nodes/{node_id}/skip"),
        ("GET", "/ws"),
    ]
    .iter()
    .map(|(m, p)| (m.to_string(), p.to_string()))
    .collect();
    assert_eq!(got, want.iter().collect(), "an assert on a text is no call");
}
