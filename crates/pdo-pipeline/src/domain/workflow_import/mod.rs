//! Claude Code workflow importer (ADR-0016).
//!
//! "Decompiles" a Claude Code dynamic workflow (`.claude/workflows/*.js`) into a
//! **draft** PDO [`PipelineDef`].
//!
//! - `agent(prompt, opts)` -> an isolated `agent` node (#653): the import maps a
//!   role, never a guess about what that role will touch.
//! - `for` / `while` **whose body contains an `agent()`** -> a `bounded` loop
//!   region; a plumbing loop (no `agent()` in the body) is left alone — the guard
//!   that prevents a phantom `bounded` (ADR-0016).
//! - `pipeline(items, s1, s2, …)` -> a `collection` loop region.
//! - `parallel([…])` -> sibling fan-out edges.
//! - `if (x OP v) { return / break }` guarding a spawn/return -> a conditional
//!   `when:` edge (ADR-0002 grammar, ADR-0011 edge-centric routing).
//! - `opts.schema = {…JSON schema…}` -> frontmatter on the node's output port.
//!
//! Prompt extraction has three tiers, referenced elsewhere as N1/N2/N3:
//! - **N1** static string / no-substitution template -> body **verbatim**.
//! - **N2** template with `${…}` -> static quasis kept verbatim, each hole a
//!   `⟨input: …⟩` marker (never a live token).
//! - **N3** no static text at all (helper return, bare identifier) -> an annotated
//!   placeholder body + a warning.
//!
//! Anything outside the recognized subset degrades to an annotated placeholder or
//! a `warnings` entry: the import must **succeed without crashing** and produce a
//! usable draft, never panic. Onboarding, not fidelity.

mod ast;
mod consts;
mod emit;
mod importer;
mod literals;
mod predicate;
mod render;
mod walk;
mod wiring;

use ast::*;
use consts::*;
use literals::*;
use predicate::*;
use render::*;
use wiring::*;

use std::collections::{HashMap, HashSet};

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;

use crate::domain::pipeline::{Diagnostic, EdgeDef, FrontmatterFieldDecl, LoopRegion, NodeDef};

/// The default iteration cap for a `bounded` region whose bound can't be
/// resolved to a literal — matches [`crate::domain::loops::DEFAULT_MAX_ITER`].
pub(super) const DEFAULT_MAX_ITER: i64 = 5;

/// Recursion / size guards. The daemon is LAN-reachable; a pathological `.js`
/// must not blow the stack or produce an unbounded pipeline.
pub(super) const MAX_DEPTH: u32 = 256;
pub(super) const MAX_NODES: usize = 512;

/// The outcome of importing a workflow `.js`.
#[derive(Debug)]
pub struct ImportResult {
    /// Pipeline display name (from `meta.name`, else the suggested/stem name).
    pub name: String,
    /// The constructed pipeline serialized to YAML. Serializing is fine here: a
    /// fresh draft has no source formatting to preserve, unlike an edited entry.
    pub yaml_text: String,
    /// node_id -> prompt body, for `library_store::pipelines::save`.
    pub prompts: HashMap<String, String>,
    pub warnings: Vec<Diagnostic>,
}

/// Parse a Claude Code workflow `.js` into a draft [`PipelineDef`] (never runs the
/// JS). `suggested_name` is the fallback pipeline name (typically the file stem)
/// used when the workflow declares no `meta.name`.
pub fn import_workflow_js(source: &str, suggested_name: &str) -> Result<ImportResult, String> {
    let allocator = Allocator::default();
    // Module JS so top-level `export const meta = …` parses (Script mode rejects
    // `export`). `.mjs()` is module JavaScript; no path needed.
    let source_type = SourceType::mjs();
    let ret = Parser::new(&allocator, source, source_type).parse();
    // Refuse a partial tree rather than best-effort over it: this input is
    // untrusted. `panicked` ⇒ the parser bailed, so there is no usable AST.
    if ret.panicked {
        return Err(format!(
            "JS parse error ({} diagnostic(s)) — the file is not valid JavaScript",
            ret.errors.len()
        ));
    }
    if let Some(other) = ret
        .errors
        .iter()
        .find(|e| !is_tolerated_parse_error(&e.to_string()))
    {
        return Err(format!("JS parse error: {other}"));
    }

    let body = &ret.program.body;

    // Must precede the walk: `max_iter`/`schema`/provenance resolve against these.
    let mut consts = Consts::default();
    collect_consts(body, &mut consts, 0);

    let name = extract_meta_name(body).unwrap_or_else(|| suggested_name.to_string());

    let mut imp = Importer::new(consts);
    let mut cursor: Cursor = vec![Pending::start()];
    imp.walk_block(body, &mut cursor, 0);
    imp.finish(&mut cursor);

    let pipeline = imp.build_pipeline(&name);
    let yaml_text = serde_yaml::to_string(&pipeline)
        .map_err(|e| format!("failed to serialize imported pipeline: {e}"))?;

    Ok(ImportResult {
        name,
        yaml_text,
        prompts: imp_take_prompts(&mut imp),
        warnings: imp.warnings,
    })
}

// `into_pipeline` consumes fields; take prompts first to avoid a partial move.
pub(super) fn imp_take_prompts(imp: &mut Importer) -> HashMap<String, String> {
    std::mem::take(&mut imp.prompts)
}

/// A recoverable parse diagnostic we tolerate: a top-level `return` (or `await`).
/// Claude Code workflow scripts execute inside the Workflow harness's async
/// function wrapper, so these are legal there even though a bare ES module flags
/// them; the AST is complete regardless. Any other diagnostic is a real error.
pub(super) fn is_tolerated_parse_error(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    lower.contains("return") || lower.contains("await")
}

pub(super) const NANOID_ALPHABET: &[u8] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
pub(super) const NANOID_LEN: usize = 8;

/// FNV-1a, matching `pipeline_migrator`. Don't swap in a random id: re-importing
/// the same source must yield the same node ids.
pub(super) fn deterministic_id(seed: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in seed.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let mut out = String::with_capacity(NANOID_LEN);
    for i in 0..NANOID_LEN {
        let idx = ((hash >> (i * 8)) & 0xFF) as usize % NANOID_ALPHABET.len();
        out.push(NANOID_ALPHABET[idx] as char);
    }
    out
}

/// Resolved constants — fully owned, so no AST borrow outlives the allocator.
#[derive(Default)]
pub(super) struct Consts {
    numbers: HashMap<String, f64>,
    /// const-name -> frontmatter map, for every top-level object literal (only
    /// consumed when referenced as `schema:`).
    schemas: HashMap<String, HashMap<String, FrontmatterFieldDecl>>,
    /// binding names whose initializer reads `args` (the run's user prompt).
    from_args: HashSet<String>,
}

/// A pending source endpoint feeding the next node's `in` port.
#[derive(Clone)]
pub(super) struct Pending {
    node: String,
    port: String,
    when: Option<serde_yaml::Value>,
    is_else: bool,
}

impl Pending {
    fn start() -> Self {
        Pending {
            node: "start".into(),
            port: "user_prompt".into(),
            when: None,
            is_else: false,
        }
    }
}

pub(super) type Cursor = Vec<Pending>;

/// A loop currently being materialized. Only the outermost real loop builds a
/// region; a break-guard inside it records its exit predicate here.
pub(super) struct LoopFrame {
    break_when: Option<serde_yaml::Value>,
    break_source: Option<String>,
}

pub(super) struct Importer {
    consts: Consts,
    nodes: Vec<NodeDef>,
    edges: Vec<EdgeDef>,
    loops: Vec<LoopRegion>,
    prompts: HashMap<String, String>,
    warnings: Vec<Diagnostic>,
    used_ids: HashSet<String>,
    ordinal: usize,
    loop_ordinal: usize,
    loop_stack: Vec<LoopFrame>,
    /// var-name -> node id, for `const x = await agent(…)` provenance.
    agent_vars: HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::pipeline::{self, LoopKind, NodeType};

    /// Import a `.js` and re-parse the produced YAML — the round-trip that a real
    /// save performs (`library_store::pipelines::save` calls `parse_pipeline`).
    fn import_and_parse(src: &str, name: &str) -> (ImportResult, pipeline::PipelineDef) {
        let result = import_workflow_js(src, name).expect("import must succeed");
        let parsed = pipeline::parse_pipeline(&result.yaml_text)
            .expect("imported pipeline YAML must parse")
            .pipeline;
        (result, parsed)
    }

    fn prompt_of<'a>(
        result: &'a ImportResult,
        parsed: &pipeline::PipelineDef,
        node_name: &str,
    ) -> &'a str {
        let id = &parsed
            .nodes
            .iter()
            .find(|n| n.name == node_name)
            .unwrap_or_else(|| panic!("no node named '{node_name}'"))
            .id;
        result
            .prompts
            .get(id)
            .unwrap_or_else(|| panic!("no prompt for node '{node_name}'"))
    }

    fn when_strings(parsed: &pipeline::PipelineDef) -> Vec<String> {
        parsed
            .edges
            .iter()
            .filter_map(|e| e.when.as_ref())
            .map(|w| serde_yaml::to_string(w).unwrap())
            .collect()
    }

    #[test]
    fn n1_static_literal_extracted_verbatim() {
        let src = r#"agent(`Do the thing exactly.`, { label: 'worker' })"#;
        let (result, parsed) = import_and_parse(src, "t");
        let body = prompt_of(&result, &parsed, "worker");
        assert_eq!(body, "Do the thing exactly.");
        assert!(!body.contains('⟨'), "no hole marker on a static prompt");
    }

    #[test]
    fn n2_template_interpolation_keeps_static_and_marks_holes() {
        let src = r#"const bug = ''; agent(`Fix ${bug} at the root now.`, { label: 'fixer' })"#;
        let (result, parsed) = import_and_parse(src, "t");
        let body = prompt_of(&result, &parsed, "fixer");
        assert!(body.contains("Fix "));
        assert!(body.contains(" at the root now."));
        assert!(body.contains("⟨input: bug⟩"));
    }

    #[test]
    fn n3_bare_helper_return_becomes_placeholder_with_warning() {
        let src = r#"const build = () => 'x'; agent(build(), { label: 'mystery' })"#;
        let (result, parsed) = import_and_parse(src, "t");
        let body = prompt_of(&result, &parsed, "mystery");
        assert!(
            body.contains("TODO") || body.contains('⟨'),
            "N3 prompt is an annotated placeholder, got: {body}"
        );
        assert!(
            !result.warnings.is_empty(),
            "N3 placeholder must raise a warning"
        );
    }

    #[test]
    fn agent_opts_effort_and_model_land_on_the_node() {
        let src = r#"agent(`work`, { label: 'w', model: 'opus', effort: 'low' }); agent(`plain`, { label: 'p' })"#;
        let (result, parsed) = import_and_parse(src, "t");
        let node = |name: &str| parsed.nodes.iter().find(|n| n.name == name).unwrap();
        // Imported model/effort land under `harnesses.claude` (claude-only import).
        let claude = |name: &str| node(name).harnesses.get("claude").cloned();
        assert_eq!(claude("w").and_then(|e| e.effort).as_deref(), Some("low"));
        assert_eq!(claude("w").and_then(|e| e.model).as_deref(), Some("opus"));
        // An agent with no effort/model stays unset — no claude entry at all.
        assert_eq!(claude("p"), None);
        assert!(
            result
                .warnings
                .iter()
                .all(|w| !w.message.contains("effort")),
            "a literal effort must not warn: {:?}",
            result.warnings
        );
        // Structural nodes never carry one.
        assert!(parsed
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::Start)
            .unwrap()
            .harnesses
            .is_empty());
    }

    #[test]
    fn computed_agent_effort_warns_instead_of_vanishing() {
        // A non-literal level can't be resolved without executing the script, so
        // it IS lost — announce it (ADR-0001) rather than dropping it in silence.
        let src = r#"const lvl = 'low'; agent(`work`, { label: 'w', effort: lvl })"#;
        let (result, parsed) = import_and_parse(src, "t");
        // A computed (non-literal) effort is lost, so the node carries no claude entry.
        assert!(parsed
            .nodes
            .iter()
            .find(|n| n.name == "w")
            .unwrap()
            .harnesses
            .is_empty());
        assert!(
            result.warnings.iter().any(|w| w.message.contains("effort")),
            "a computed effort must raise a warning: {:?}",
            result.warnings
        );
    }

    #[test]
    fn while_without_agent_is_not_a_loop() {
        let src = r#"let n = 0; while (n < 3) { n = n + 1 } agent(`work`, { label: 'w' })"#;
        let (_result, parsed) = import_and_parse(src, "t");
        assert!(
            parsed.loops.is_empty(),
            "a plumbing while must not materialize a bounded region"
        );
    }

    #[test]
    fn for_with_agent_becomes_bounded() {
        let src = r#"for (let i = 1; i <= 3; i++) { agent(`lap`, { label: 'w' }) }"#;
        let (_result, parsed) = import_and_parse(src, "t");
        let bounded: Vec<_> = parsed
            .loops
            .iter()
            .filter(|l| l.kind == LoopKind::Bounded)
            .collect();
        assert_eq!(bounded.len(), 1);
        assert_eq!(
            bounded[0].max_iter,
            Some(serde_yaml::Value::Number(serde_yaml::Number::from(3)))
        );
    }

    #[test]
    fn pipeline_call_becomes_collection() {
        let src = r#"const items = []; const r = await pipeline(items, (p) => agent(`per-item`, { label: 'worker' }))"#;
        let (_result, parsed) = import_and_parse(src, "t");
        let collection: Vec<_> = parsed
            .loops
            .iter()
            .filter(|l| l.kind == LoopKind::Collection)
            .collect();
        assert_eq!(collection.len(), 1, "pipeline() -> one collection region");
        assert!(collection[0].over.is_some(), "collection carries an `over`");
    }

    /// #655/ADR-0060: every imported role is an isolated `agent`, and the YAML
    /// says so out loud — a foreign workflow arrives without an opinion on
    /// worktrees, and inventing one for it is the guesswork #653 retired.
    #[test]
    fn imported_roles_are_isolated_agents_and_the_yaml_says_so() {
        let src = r#"agent(`Read the docs and summarise.`, { label: 'reader' })"#;
        let (result, parsed) = import_and_parse(src, "t");
        let node = parsed.nodes.iter().find(|n| n.name == "reader").unwrap();
        assert_eq!(node.node_type, NodeType::Agent);
        assert_eq!(node.isolated_worktree, Some(true));
        assert!(
            result.yaml_text.contains("isolated_worktree: true"),
            "the emitted document must state the isolation:\n{}",
            result.yaml_text
        );
    }

    /// The four signals the pre-#653 importer used to sniff, plus the one #655
    /// adds: a `collection` region. None of them may move the isolation — a
    /// read-only-sounding prompt is still an isolated Agent.
    #[test]
    fn import_never_infers_isolation_from_prompt_name_outputs_or_collection() {
        for (label, src) in [
            (
                "a read-only-sounding prompt",
                r#"agent(`Only read the docs. Do not write, edit or commit anything.`, { label: 'reader' })"#,
            ),
            (
                "a documentation-sounding name",
                r#"agent(`work`, { label: 'docs-writer' })"#,
            ),
            (
                "a schema'd output",
                r#"agent(`work`, { label: 'reader', schema: { type: 'object', properties: { verdict: { type: 'string' } } } })"#,
            ),
            (
                "a collection member",
                r#"const items = []; await pipeline(items, (p) => agent(`per-item`, { label: 'reader' }))"#,
            ),
            (
                "an annotated placeholder (N3)",
                r#"const build = () => 'x'; agent(build(), { label: 'reader' })"#,
            ),
        ] {
            let (_result, parsed) = import_and_parse(src, "t");
            let node = parsed
                .nodes
                .iter()
                .find(|n| n.node_type == NodeType::Agent)
                .unwrap_or_else(|| panic!("{label}: no agent node"));
            assert_eq!(node.node_type, NodeType::Agent, "{label}");
            assert_eq!(
                node.isolated_worktree,
                Some(true),
                "{label} must not move the isolation"
            );
        }
    }

    #[test]
    fn if_equality_becomes_when_edge() {
        let src = r#"const r = await agent(`triage`, { label: 'triage' }); if (r.verdict === 'Bug') { return {} }"#;
        let (_result, parsed) = import_and_parse(src, "t");
        let whens = when_strings(&parsed);
        assert!(
            whens
                .iter()
                .any(|w| w.contains("verdict") && w.contains("eq") && w.contains("Bug")),
            "if (r.verdict === 'Bug') -> when {{ verdict: {{ eq: Bug }} }}, got {whens:?}"
        );
    }

    #[test]
    fn bare_identifier_equality_maps_field_to_itself() {
        let src = r#"agent(`a`, { label: 'a' }); if (status === 'done') { return {} }"#;
        let (_result, parsed) = import_and_parse(src, "t");
        let whens = when_strings(&parsed);
        assert!(
            whens
                .iter()
                .any(|w| w.contains("status") && w.contains("done")),
            "if (status === 'done') -> when {{ status: {{ eq: done }} }}, got {whens:?}"
        );
    }

    #[test]
    fn json_schema_becomes_port_frontmatter() {
        let src = r#"agent(`triage`, { label: 'triage', schema: {
            type: 'object', required: ['verdict'],
            properties: {
                verdict: { type: 'string', enum: ['Pass', 'Fail'] },
                note: { type: 'string' },
                count: { type: 'integer' },
            },
        } })"#;
        let (_result, parsed) = import_and_parse(src, "t");
        let node = parsed.nodes.iter().find(|n| n.name == "triage").unwrap();
        let fm = node.outputs[0].frontmatter.as_ref().expect("frontmatter");
        assert_eq!(fm.get("verdict").unwrap().field_type, "enum");
        assert_eq!(
            fm.get("verdict").unwrap().allowed.as_ref().unwrap(),
            &vec!["Pass".to_string(), "Fail".to_string()]
        );
        assert_eq!(fm.get("note").unwrap().field_type, "string");
        assert_eq!(fm.get("count").unwrap().field_type, "int");
    }

    #[test]
    fn parallel_fans_out_to_siblings() {
        let src = r#"await parallel([() => agent(`a`, { label: 'a' }), () => agent(`b`, { label: 'b' })])"#;
        let (_result, parsed) = import_and_parse(src, "t");
        let a = &parsed.nodes.iter().find(|n| n.name == "a").unwrap().id;
        let b = &parsed.nodes.iter().find(|n| n.name == "b").unwrap().id;
        // Both siblings entered from `start` (the shared upstream).
        assert!(parsed
            .edges
            .iter()
            .any(|e| e.source.node == "start" && e.target.node == *a));
        assert!(parsed
            .edges
            .iter()
            .any(|e| e.source.node == "start" && e.target.node == *b));
    }

    #[test]
    fn meta_name_wins_over_suggested() {
        let src = r#"export const meta = { name: 'my-flow' }; agent(`x`, { label: 'w' })"#;
        let result = import_workflow_js(src, "fallback").unwrap();
        assert_eq!(result.name, "my-flow");
    }

    #[test]
    fn no_meta_falls_back_to_suggested_name() {
        let src = r#"agent(`x`, { label: 'w' })"#;
        let result = import_workflow_js(src, "stem-name").unwrap();
        assert_eq!(result.name, "stem-name");
    }

    #[test]
    fn invalid_js_is_rejected() {
        let err = import_workflow_js("const x = = ;", "t").unwrap_err();
        assert!(err.to_lowercase().contains("parse"), "got: {err}");
    }
}
