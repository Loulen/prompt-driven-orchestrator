//! Claude Code workflow importer (#155 / ADR-0016).
//!
//! "Decompiles" a Claude Code dynamic workflow (`.claude/workflows/*.js`) into a
//! **draft** PDO [`PipelineDef`] — never executing the imported JS. The `.js` is
//! parsed to an AST with `oxc` (no V8/node subprocess: the daemon binds `0.0.0.0`,
//! so running an imported file's code would be an RCE, cf. #260) and the
//! recognized idioms are rewired:
//!
//! - `agent(prompt, opts)` -> a regular node (`doc-only`, upgraded to
//!   `code-mutating` on strong mutation keywords / `isolation: 'worktree'`).
//! - `for` / `while` **whose body contains an `agent()`** -> a `bounded` loop
//!   region; a plumbing loop (no `agent()` in the body) is left alone — the guard
//!   that prevents a phantom `bounded` (ADR-0016).
//! - `pipeline(items, s1, s2, …)` -> a `collection` loop region.
//! - `parallel([…])` -> sibling fan-out edges.
//! - `if (x OP v) { return / break }` guarding a spawn/return -> a conditional
//!   `when:` edge (ADR-0002 grammar, ADR-0011 edge-centric routing).
//! - `opts.schema = {…JSON schema…}` -> frontmatter on the node's output port.
//!
//! Prompt extraction is the core value (three tiers):
//! - **N1** static string / no-substitution template -> body **verbatim**.
//! - **N2** template with `${…}` -> static quasis kept verbatim, each hole a
//!   `⟨input: …⟩` marker (never a live token).
//! - **N3** no static text at all (helper return, bare identifier) -> an annotated
//!   placeholder body + a warning.
//!
//! Anything outside the recognized subset (nested loops, budget guards,
//! `try/finally`, cross-lap accumulation) degrades to an annotated placeholder or
//! a `warnings` entry — the import must **succeed without crashing** and produce a
//! usable draft, never panic. Onboarding, not fidelity: "import the wiring, flag
//! the rest".

use std::collections::{HashMap, HashSet};

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    BinaryOperator, CallExpression, Declaration, Expression, LogicalOperator, ObjectExpression,
    ObjectPropertyKind, Statement,
};
use oxc_parser::Parser;
use oxc_span::SourceType;

use crate::pipeline::{
    Diagnostic, EdgeDef, EdgeEndpoint, FrontmatterFieldDecl, LoopKind, LoopRegion, NodeDef,
    NodeType, PipelineDef, Port, Severity, ViewPosition,
};

/// The default iteration cap for a `bounded` region whose bound can't be
/// resolved to a literal — matches [`crate::loop_region::DEFAULT_MAX_ITER`].
const DEFAULT_MAX_ITER: i64 = 5;

/// Recursion / size guards. The daemon is LAN-reachable (#260); a pathological
/// `.js` must not blow the stack or produce an unbounded pipeline.
const MAX_DEPTH: u32 = 256;
const MAX_NODES: usize = 512;

/// The outcome of importing a workflow `.js`.
#[derive(Debug)]
pub struct ImportResult {
    /// Pipeline display name (from `meta.name`, else the suggested/stem name).
    pub name: String,
    /// The constructed pipeline serialized to YAML (a fresh draft — not a source
    /// to preserve, so serializing is correct here, unlike editing an entry).
    pub yaml_text: String,
    /// node_id -> prompt body, for `library_store::pipelines::save`.
    pub prompts: HashMap<String, String>,
    /// Lossy-translation diagnostics ("idiom not mapped -> placeholder", etc.).
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
    // Security-sensible import: refuse anything without a usable AST rather than
    // best-effort over a partial tree. `panicked` ⇒ the parser bailed (no tree).
    if ret.panicked {
        return Err(format!(
            "JS parse error ({} diagnostic(s)) — the file is not valid JavaScript",
            ret.errors.len()
        ));
    }
    // A Claude Code workflow runs inside the harness's async wrapper, so a
    // top-level `return`/`await` is legal there even though a bare ES module
    // flags it (recoverable — the AST is still complete). Tolerate exactly that
    // class; any *other* parse error means the file is not a workflow we can read.
    if let Some(other) = ret
        .errors
        .iter()
        .find(|e| !is_tolerated_parse_error(&e.to_string()))
    {
        return Err(format!("JS parse error: {other}"));
    }

    let body = &ret.program.body;

    // Pass 1 — resolve top-level literal constants (numbers, JSON-schema objects,
    // args-derived bindings) so `max_iter`/`schema`/provenance can be resolved.
    let mut consts = Consts::default();
    collect_consts(body, &mut consts, 0);

    let name = extract_meta_name(body).unwrap_or_else(|| suggested_name.to_string());

    // Pass 2 — walk the program, materializing nodes/edges/loops/prompts.
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

// `into_pipeline` consumes fields; take prompts before that to avoid a partial
// move dance. Small helper keeps the public fn readable.
fn imp_take_prompts(imp: &mut Importer) -> HashMap<String, String> {
    std::mem::take(&mut imp.prompts)
}

/// A recoverable parse diagnostic we tolerate: a top-level `return` (or `await`).
/// Claude Code workflow scripts execute inside the Workflow harness's async
/// function wrapper, so these are legal there even though a bare ES module flags
/// them; the AST is complete regardless. Any other diagnostic is a real error.
fn is_tolerated_parse_error(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    lower.contains("return") || lower.contains("await")
}

// ---------------------------------------------------------------------------
// deterministic id (FNV-1a, copied from pipeline_migrator so a re-import of the
// same source is idempotent — same seed => same 8-char id).
// ---------------------------------------------------------------------------

const NANOID_ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const NANOID_LEN: usize = 8;

fn deterministic_id(seed: &str) -> String {
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

// ---------------------------------------------------------------------------
// Resolved constants (fully owned — no AST borrows stored).
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Consts {
    numbers: HashMap<String, f64>,
    /// const-name -> frontmatter map, for every top-level object literal (only
    /// consumed when referenced as `schema:`).
    schemas: HashMap<String, HashMap<String, FrontmatterFieldDecl>>,
    /// binding names whose initializer reads `args` (the run's user prompt).
    from_args: HashSet<String>,
}

fn collect_consts(stmts: &[Statement], consts: &mut Consts, depth: u32) {
    if depth > MAX_DEPTH {
        return;
    }
    for stmt in stmts {
        match stmt {
            Statement::VariableDeclaration(decl) => {
                for d in &decl.declarations {
                    let Some(name) = binding_name(&d.id) else {
                        continue;
                    };
                    let Some(init) = &d.init else { continue };
                    let init = init.without_parentheses();
                    if let Some(n) = resolve_number(init, consts) {
                        consts.numbers.insert(name.to_string(), n);
                    }
                    if let Expression::ObjectExpression(obj) = init {
                        consts
                            .schemas
                            .insert(name.to_string(), object_to_frontmatter(obj));
                    }
                    if expr_mentions_ident(init, "args", 0) {
                        consts.from_args.insert(name.to_string());
                    }
                }
            }
            // `export const meta = …` etc.
            Statement::ExportNamedDeclaration(e) => {
                if let Some(Declaration::VariableDeclaration(decl)) = &e.declaration {
                    for d in &decl.declarations {
                        if let (Some(name), Some(init)) = (binding_name(&d.id), &d.init) {
                            let init = init.without_parentheses();
                            if let Some(n) = resolve_number(init, consts) {
                                consts.numbers.insert(name.to_string(), n);
                            }
                            if let Expression::ObjectExpression(obj) = init {
                                consts
                                    .schemas
                                    .insert(name.to_string(), object_to_frontmatter(obj));
                            }
                        }
                    }
                }
            }
            // Descend into blocks so consts declared inside `try { … }` etc. still
            // resolve (sandcastle wraps its orchestration in a try/finally).
            Statement::BlockStatement(b) => collect_consts(&b.body, consts, depth + 1),
            Statement::TryStatement(t) => {
                collect_consts(&t.block.body, consts, depth + 1);
                if let Some(f) = &t.finalizer {
                    collect_consts(&f.body, consts, depth + 1);
                }
                if let Some(h) = &t.handler {
                    collect_consts(&h.body.body, consts, depth + 1);
                }
            }
            Statement::ForStatement(f) => {
                if let Statement::BlockStatement(b) = &f.body {
                    collect_consts(&b.body, consts, depth + 1);
                }
            }
            Statement::WhileStatement(w) => {
                if let Statement::BlockStatement(b) = &w.body {
                    collect_consts(&b.body, consts, depth + 1);
                }
            }
            Statement::IfStatement(i) => {
                if let Statement::BlockStatement(b) = &i.consequent {
                    collect_consts(&b.body, consts, depth + 1);
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// The importer: owns all output; reads `Consts` by value.
// ---------------------------------------------------------------------------

/// A pending source endpoint feeding the next node's `in` port.
#[derive(Clone)]
struct Pending {
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

type Cursor = Vec<Pending>;

/// A loop currently being materialized. Only the outermost real loop builds a
/// region; a break-guard inside it records its exit predicate here.
struct LoopFrame {
    break_when: Option<serde_yaml::Value>,
    break_source: Option<String>,
}

struct Importer {
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

impl Importer {
    fn new(consts: Consts) -> Self {
        Importer {
            consts,
            nodes: Vec::new(),
            edges: Vec::new(),
            loops: Vec::new(),
            prompts: HashMap::new(),
            warnings: Vec::new(),
            used_ids: HashSet::new(),
            ordinal: 0,
            loop_ordinal: 0,
            loop_stack: Vec::new(),
            agent_vars: HashMap::new(),
        }
    }

    fn warn(&mut self, message: impl Into<String>) {
        self.warnings.push(Diagnostic {
            severity: Severity::Warning,
            message: message.into(),
        });
    }

    fn unique_id(&mut self, seed: &str) -> String {
        let base = if seed.is_empty() { "node" } else { seed };
        let mut id = deterministic_id(base);
        let mut n = 2u32;
        while self.used_ids.contains(&id) {
            id = deterministic_id(&format!("{base}-{n}"));
            n += 1;
        }
        self.used_ids.insert(id.clone());
        id
    }

    // --- statement walking ------------------------------------------------

    fn walk_block(&mut self, stmts: &[Statement], cursor: &mut Cursor, depth: u32) {
        if depth > MAX_DEPTH {
            return;
        }
        for stmt in stmts {
            self.walk_stmt(stmt, cursor, depth);
        }
    }

    fn walk_stmt(&mut self, stmt: &Statement, cursor: &mut Cursor, depth: u32) {
        if depth > MAX_DEPTH || self.nodes.len() >= MAX_NODES {
            return;
        }
        match stmt {
            Statement::ExportNamedDeclaration(e) => {
                if let Some(Declaration::VariableDeclaration(decl)) = &e.declaration {
                    self.walk_var_decl(decl, cursor, depth);
                }
            }
            Statement::VariableDeclaration(decl) => self.walk_var_decl(decl, cursor, depth),
            Statement::ExpressionStatement(es) => {
                self.walk_value_expr(&es.expression, None, cursor, depth);
            }
            Statement::ForStatement(f) => {
                let bound = resolve_for_bound(f.test.as_ref(), &self.consts);
                self.walk_loop(&f.body, bound, cursor, depth);
            }
            Statement::WhileStatement(w) => {
                let bound = resolve_test_bound(Some(&w.test), &self.consts);
                self.walk_loop(&w.body, bound, cursor, depth);
            }
            Statement::DoWhileStatement(w) => {
                let bound = resolve_test_bound(Some(&w.test), &self.consts);
                self.walk_loop(&w.body, bound, cursor, depth);
            }
            Statement::IfStatement(i) => self.walk_if(i, cursor, depth),
            Statement::TryStatement(t) => {
                self.warn("`try/finally` (cleanup garanti) sans équivalent structurel — le corps est aplati, la garantie de nettoyage est perdue (idiome hors périmètre v1)");
                self.walk_block(&t.block.body, cursor, depth + 1);
                if let Some(f) = &t.finalizer {
                    self.walk_block(&f.body, cursor, depth + 1);
                }
            }
            Statement::BlockStatement(b) => self.walk_block(&b.body, cursor, depth + 1),
            Statement::ReturnStatement(_) => {
                // A bare top-level return is the workflow's return value — no node.
            }
            _ => {}
        }
    }

    fn walk_var_decl(
        &mut self,
        decl: &oxc_ast::ast::VariableDeclaration,
        cursor: &mut Cursor,
        depth: u32,
    ) {
        for d in &decl.declarations {
            let Some(init) = &d.init else { continue };
            let bind = binding_name(&d.id).map(|s| s.to_string());
            self.walk_value_expr(init, bind.as_deref(), cursor, depth);
        }
    }

    /// Handle an expression in value position (a statement's expression, or a
    /// declarator initializer). `bind` is the JS variable it's assigned to, if any.
    fn walk_value_expr(
        &mut self,
        expr: &Expression,
        bind: Option<&str>,
        cursor: &mut Cursor,
        depth: u32,
    ) {
        let inner = unwrap_expr(expr);
        if let Some(call) = as_agent_call(inner) {
            let id = self.emit_agent(call, cursor);
            if let Some(name) = bind {
                self.agent_vars.insert(name.to_string(), id);
            }
            return;
        }
        if let Some(call) = as_named_call(inner, "pipeline") {
            self.handle_pipeline(call, cursor, depth);
            return;
        }
        if let Some(call) = as_named_call(inner, "parallel") {
            self.handle_parallel(call, cursor, depth);
        }
        // Anything else (log/phase/assignment/plain data) contributes no node.
    }

    // --- agent node -------------------------------------------------------

    fn emit_agent(&mut self, call: &CallExpression, cursor: &mut Cursor) -> String {
        let opts = call.arguments.get(1).and_then(|a| a.as_expression());
        let label = opts.and_then(|o| object_prop(o, "label"));
        let label_static = label.and_then(static_prefix_of);
        let model = opts
            .and_then(|o| object_prop(o, "model"))
            .and_then(string_literal_value);
        let isolation_wt = opts
            .and_then(|o| object_prop(o, "isolation"))
            .and_then(string_literal_value)
            .as_deref()
            == Some("worktree");

        let seed = label_static
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("node-{}", self.ordinal));
        let id = self.unique_id(&seed);
        self.ordinal += 1;

        let display_name = label_static
            .as_deref()
            .map(clean_name)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| id.clone());

        // Prompt extraction (N1/N2/N3).
        let prompt_arg = call.arguments.first().and_then(|a| a.as_expression());
        let (prompt_body, has_static, refs) = match prompt_arg {
            Some(e) => render_prompt(e),
            None => (String::new(), false, Vec::new()),
        };
        let prompt_body = if has_static {
            prompt_body
        } else {
            self.warn(format!(
                "nœud '{display_name}': prompt construit dynamiquement (helper nu / identifiant) — placeholder annoté à rédiger (N3, hors périmètre v1 : résolution one-hop du helper)"
            ));
            if prompt_body.is_empty() {
                "⟨TODO: prompt construit dynamiquement — rédige le rôle de ce nœud⟩".to_string()
            } else {
                prompt_body
            }
        };

        let node_type = infer_node_type(&prompt_body, &display_name, isolation_wt);

        // Output port frontmatter from `opts.schema` (const ref or inline object).
        let frontmatter = opts.and_then(|o| object_prop(o, "schema")).and_then(|s| {
            let s = s.without_parentheses();
            match s {
                Expression::Identifier(id) => self.consts.schemas.get(id.name.as_str()).cloned(),
                Expression::ObjectExpression(obj) => Some(object_to_frontmatter(obj)),
                _ => None,
            }
        });
        let frontmatter = frontmatter.filter(|m| !m.is_empty());

        let mut out_port = Port {
            name: "out".into(),
            repeated: false,
            side: None,
            port_type: crate::pipeline::PortType::Markdown,
            frontmatter: None,
            when: None,
            description: None,
        };
        out_port.frontmatter = frontmatter;

        let node = NodeDef {
            id: id.clone(),
            name: display_name,
            node_type,
            inputs: vec![plain_port("in")],
            outputs: vec![out_port],
            interactive: false,
            view: Some(ViewPosition {
                x: 320.0,
                y: (self.ordinal as f64) * 140.0,
            }),
            max_iter: None,
            over: None,
            model,
        };
        self.nodes.push(node);
        self.prompts.insert(id.clone(), prompt_body);

        // Wire chain edges from the cursor into this node.
        for p in cursor.iter() {
            self.add_edge(&p.node, &p.port, &id, "in", p.when.clone(), p.is_else);
        }

        // Best-effort provenance edges from interpolation holes (ADR-0011): a
        // top-level arg (`bugReport`) => an edge from `start`; a var bound to an
        // upstream `agent()` => an edge from that node. Deduped per (source,target)
        // pair so we never contradict a chain/loop edge.
        for r in &refs {
            let root = r.split(['.', '[']).next().unwrap_or(r).trim().to_string();
            if root.is_empty() {
                continue;
            }
            if self.consts.from_args.contains(&root) || root == "args" {
                self.add_provenance_edge("start", "user_prompt", &id);
            } else if let Some(src) = self.agent_vars.get(&root).cloned() {
                self.add_provenance_edge(&src, "out", &id);
            }
        }

        *cursor = vec![Pending {
            node: id.clone(),
            port: "out".into(),
            when: None,
            is_else: false,
        }];
        id
    }

    // --- loops ------------------------------------------------------------

    fn walk_loop(&mut self, body: &Statement, bound: MaxIter, cursor: &mut Cursor, depth: u32) {
        if !stmt_has_agent(body, 0) {
            // Plumbing loop (double-decode of `args`, `JSON.parse` retry, …) — not
            // a pipeline loop. The anti-phantom-`bounded` guard (ADR-0016).
            return;
        }
        if !self.loop_stack.is_empty() {
            // Nested loop: ADR-0011 mandates flat iteration. Flatten the body inline
            // (so its agents still appear) but emit no nested region.
            self.warn("boucle imbriquée non supportée (ADR-0011 : itération plate) — le corps est aplati sans région dédiée ; à recâbler à la main");
            self.walk_stmt_loop_body(body, cursor, depth + 1);
            return;
        }

        let entry_before = self.nodes.len();
        self.loop_stack.push(LoopFrame {
            break_when: None,
            break_source: None,
        });
        self.walk_stmt_loop_body(body, cursor, depth + 1);
        let frame = self.loop_stack.pop().expect("loop frame pushed above");

        let members: Vec<String> = self.nodes[entry_before..]
            .iter()
            .map(|n| n.id.clone())
            .collect();
        if members.is_empty() {
            return;
        }
        let entry = members[0].clone();

        let max_iter = match bound {
            MaxIter::Value(v) => Some(serde_yaml::Value::Number(serde_yaml::Number::from(v))),
            MaxIter::Unresolved => {
                self.warn(format!(
                    "borne de boucle non résolue en littéral — défaut max_iter={DEFAULT_MAX_ITER} appliqué (ADR-0011)"
                ));
                Some(serde_yaml::Value::Number(serde_yaml::Number::from(
                    DEFAULT_MAX_ITER,
                )))
            }
        };

        let region_id = {
            self.loop_ordinal += 1;
            let seed = format!("loop-{}", self.loop_ordinal);
            self.unique_id(&seed)
        };
        self.loops.push(LoopRegion {
            id: region_id,
            kind: LoopKind::Bounded,
            members,
            max_iter,
            over: None,
        });

        // Exit / back-edge wiring (mirrors `pipeline_migrator::dissolve_loops`).
        let exit_source = frame.break_source.unwrap_or_else(|| {
            self.nodes
                .last()
                .map(|n| n.id.clone())
                .unwrap_or_else(|| entry.clone())
        });
        if let Some(when) = frame.break_when {
            // Continuation back-edge: loop again while the break guard is false.
            self.add_edge(&exit_source, "out", &entry, "in", None, true);
            *cursor = vec![Pending {
                node: exit_source,
                port: "out".into(),
                when: Some(when),
                is_else: false,
            }];
        } else {
            // Unconditional loop body: leave from the last member, no guard.
            *cursor = vec![Pending {
                node: exit_source,
                port: "out".into(),
                when: None,
                is_else: false,
            }];
        }
    }

    /// Walk a loop body statement, routing a top-level `if (...) break` to the
    /// current loop frame rather than emitting an exit-to-end edge.
    fn walk_stmt_loop_body(&mut self, body: &Statement, cursor: &mut Cursor, depth: u32) {
        match body {
            Statement::BlockStatement(b) => {
                for s in &b.body {
                    self.walk_loop_inner(s, cursor, depth);
                }
            }
            other => self.walk_loop_inner(other, cursor, depth),
        }
    }

    fn walk_loop_inner(&mut self, stmt: &Statement, cursor: &mut Cursor, depth: u32) {
        if let Statement::IfStatement(i) = stmt {
            let exit = guarded_exit(&i.consequent);
            if exit == GuardExit::Break && !if_has_agent(i) {
                // Record the break predicate as the loop's exit guard.
                if let Some(frame) = self.loop_stack.last_mut() {
                    if frame.break_when.is_none() {
                        if let Some(pred) = extract_predicate(&i.test) {
                            frame.break_when = Some(pred.when_value());
                            // Resolve the break source from the predicate object.
                            let src = pred
                                .object
                                .as_ref()
                                .and_then(|o| self.agent_vars.get(o).cloned());
                            frame.break_source = src;
                        }
                    }
                }
                return;
            }
        }
        self.walk_stmt(stmt, cursor, depth);
    }

    // --- conditional edges (`if` guarding a return) -----------------------

    fn walk_if(&mut self, i: &oxc_ast::ast::IfStatement, cursor: &mut Cursor, depth: u32) {
        let exit = guarded_exit(&i.consequent);
        if exit == GuardExit::Return && !if_has_agent(i) {
            match extract_predicate(&i.test) {
                Some(pred) => {
                    let when = pred.when_value();
                    // Exit-to-end edge from each current source, guarded by the
                    // predicate; the fall-through continuation becomes `else`.
                    let sources = cursor.clone();
                    for p in &sources {
                        self.add_edge(&p.node, &p.port, "end", "result", Some(when.clone()), false);
                    }
                    for p in cursor.iter_mut() {
                        p.when = None;
                        p.is_else = true;
                    }
                }
                None => {
                    self.warn("garde `if (...) return` dont le prédicat n'est pas mappable sur un champ de frontmatter / iter / variable — ignorée (routing conditionnel non matérialisé)");
                }
            }
            return;
        }
        // `if` guarding a spawn, or with no recognizable exit: flatten both
        // branches inline (best-effort) so their agents still appear.
        if if_has_agent(i) {
            self.walk_stmt(&i.consequent, cursor, depth + 1);
            if let Some(alt) = &i.alternate {
                self.walk_stmt(alt, cursor, depth + 1);
            }
        }
    }

    // --- pipeline() / parallel() -----------------------------------------

    fn handle_pipeline(&mut self, call: &CallExpression, cursor: &mut Cursor, _depth: u32) {
        // pipeline(items, stage1, stage2, …): each stage is a callback whose body
        // contains the per-item agent(s). Collection region over `items`.
        let over = call
            .arguments
            .first()
            .and_then(|a| a.as_expression())
            .and_then(|e| match unwrap_expr(e) {
                Expression::Identifier(id) => Some(id.name.as_str().to_string()),
                _ => None,
            })
            .unwrap_or_else(|| "items".to_string());

        let member_start = self.nodes.len();
        for stage in call.arguments.iter().skip(1) {
            let Some(expr) = stage.as_expression() else {
                continue;
            };
            let mut agents: Vec<&CallExpression> = Vec::new();
            collect_agents_in_expr(expr, &mut agents, 0);
            for a in agents {
                self.emit_agent(a, cursor);
            }
        }
        let members: Vec<String> = self.nodes[member_start..]
            .iter()
            .map(|n| n.id.clone())
            .collect();
        if members.is_empty() {
            self.warn("`pipeline(...)` sans `agent()` détectable dans les stages — aucune région collection créée");
            return;
        }
        self.warn(format!(
            "`pipeline(...)` -> région collection (fan-out par item) ; `over: {over}` inféré best-effort — vérifie le champ liste sur le canvas"
        ));
        self.loop_ordinal += 1;
        let seed = format!("collection-{}", self.loop_ordinal);
        let region_id = self.unique_id(&seed);
        self.loops.push(LoopRegion {
            id: region_id,
            kind: LoopKind::Collection,
            members,
            max_iter: None,
            over: Some(over),
        });
    }

    fn handle_parallel(&mut self, call: &CallExpression, cursor: &mut Cursor, _depth: u32) {
        // parallel([thunk, …]) / parallel(arr.map(x => () => agent(…))): fan the
        // upstream out to every agent found; downstream fans in from all of them.
        let entry_sources = cursor.clone();
        let mut agents: Vec<&CallExpression> = Vec::new();
        if let Some(first) = call.arguments.first().and_then(|a| a.as_expression()) {
            collect_agents_in_expr(first, &mut agents, 0);
        }
        if agents.is_empty() {
            return;
        }
        let mut new_cursor: Cursor = Vec::new();
        let mut any_mutating = false;
        for a in agents {
            // Each sibling is entered from the same upstream sources (fan-out).
            let mut branch_cursor = entry_sources.clone();
            let id = self.emit_agent(a, &mut branch_cursor);
            if self.nodes.last().map(|n| n.node_type == NodeType::CodeMutating) == Some(true) {
                any_mutating = true;
            }
            new_cursor.push(Pending {
                node: id,
                port: "out".into(),
                when: None,
                is_else: false,
            });
        }
        if any_mutating {
            self.warn("`parallel(...)` avec des nœuds code-mutating — envisage un nœud Merge en aval (lint info-only ADR-0006, pas d'auto-insertion)");
        }
        *cursor = new_cursor;
    }

    // --- edges / finalize -------------------------------------------------

    fn add_edge(
        &mut self,
        src_node: &str,
        src_port: &str,
        tgt_node: &str,
        tgt_port: &str,
        when: Option<serde_yaml::Value>,
        is_else: bool,
    ) {
        let dup = self.edges.iter().any(|e| {
            e.source.node == src_node
                && e.source.port == src_port
                && e.target.node == tgt_node
                && e.target.port == tgt_port
                && e.when == when
                && e.is_else == is_else
        });
        if dup {
            return;
        }
        self.edges.push(EdgeDef {
            source: EdgeEndpoint {
                node: src_node.to_string(),
                port: src_port.to_string(),
            },
            target: EdgeEndpoint {
                node: tgt_node.to_string(),
                port: tgt_port.to_string(),
            },
            reason: None,
            when,
            is_else,
            repeated: false,
            mode: None,
            waypoints: None,
            target_side: None,
        });
    }

    /// Add a provenance edge only when no edge already connects the two nodes (in
    /// either port/guard form) — avoids contradicting a chain/loop edge.
    fn add_provenance_edge(&mut self, src_node: &str, src_port: &str, tgt_node: &str) {
        if src_node == tgt_node {
            return;
        }
        let exists = self
            .edges
            .iter()
            .any(|e| e.source.node == src_node && e.target.node == tgt_node);
        if exists {
            return;
        }
        self.add_edge(src_node, src_port, tgt_node, "in", None, false);
    }

    /// Connect any dangling cursor endpoints to `end`, so the pipeline is complete.
    fn finish(&mut self, cursor: &mut Cursor) {
        let pending = std::mem::take(cursor);
        if self.nodes.is_empty() {
            // No agents at all — a straight start -> end draft.
            self.add_edge("start", "user_prompt", "end", "result", None, false);
            return;
        }
        for p in pending {
            self.add_edge(&p.node, &p.port, "end", "result", p.when, p.is_else);
        }
    }

    fn build_pipeline(&self, name: &str) -> PipelineDef {
        let mut nodes = Vec::with_capacity(self.nodes.len() + 2);
        nodes.push(start_node());
        nodes.extend(self.nodes.iter().cloned());
        nodes.push(end_node(self.nodes.len()));

        PipelineDef {
            name: name.to_string(),
            version: Some("1.0".into()),
            variables: HashMap::new(),
            nodes,
            edges: self.edges.clone(),
            loops: self.loops.clone(),
            prompt_required: true,
        }
    }
}

// ---------------------------------------------------------------------------
// node builders
// ---------------------------------------------------------------------------

fn plain_port(name: &str) -> Port {
    Port {
        name: name.to_string(),
        repeated: false,
        side: None,
        port_type: crate::pipeline::PortType::Markdown,
        frontmatter: None,
        when: None,
        description: None,
    }
}

fn start_node() -> NodeDef {
    NodeDef {
        id: "start".into(),
        name: "Start".into(),
        node_type: NodeType::Start,
        inputs: vec![],
        outputs: vec![plain_port("user_prompt")],
        interactive: false,
        view: Some(ViewPosition { x: 320.0, y: 0.0 }),
        max_iter: None,
        over: None,
        model: None,
    }
}

fn end_node(agent_count: usize) -> NodeDef {
    NodeDef {
        id: "end".into(),
        name: "End".into(),
        node_type: NodeType::End,
        inputs: vec![plain_port("result")],
        outputs: vec![],
        interactive: false,
        view: Some(ViewPosition {
            x: 320.0,
            y: (agent_count as f64 + 2.0) * 140.0,
        }),
        max_iter: None,
        over: None,
        model: None,
    }
}

fn infer_node_type(prompt: &str, name: &str, isolation_worktree: bool) -> NodeType {
    if isolation_worktree {
        return NodeType::CodeMutating;
    }
    let hay = format!("{} {}", name.to_lowercase(), prompt.to_lowercase());
    const MUTATION_KW: &[&str] = &[
        "commit",
        "git add",
        "git merge",
        "implémente",
        "implemente",
        "implement",
        "worktree",
    ];
    if MUTATION_KW.iter().any(|k| hay.contains(k)) {
        NodeType::CodeMutating
    } else {
        NodeType::DocOnly
    }
}

// ---------------------------------------------------------------------------
// meta name
// ---------------------------------------------------------------------------

fn extract_meta_name(stmts: &[Statement]) -> Option<String> {
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
                    if let Some(name) = object_prop_obj(obj, "name").and_then(string_literal_value) {
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

// ---------------------------------------------------------------------------
// prompt rendering (N1/N2/N3)
// ---------------------------------------------------------------------------

/// Render a prompt expression. Returns `(body, has_static, interpolation_refs)`
/// where `has_static` is true if any static string/quasi text was found and
/// `interpolation_refs` names the `${…}` holes (for provenance edges).
fn render_prompt(expr: &Expression) -> (String, bool, Vec<String>) {
    let mut out = String::new();
    let mut has_static = false;
    let mut refs = Vec::new();
    render_into(expr, &mut out, &mut has_static, &mut refs, 0);
    (out, has_static, refs)
}

fn render_into(
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
            out.push_str(&format!("⟨TODO: prompt dynamique via {desc}()⟩"));
        }
        other => {
            let desc = describe_expr(other);
            out.push_str(&format!("⟨input: {desc}⟩"));
            refs.push(desc);
        }
    }
}

fn is_empty_string_literal(expr: &Expression) -> bool {
    matches!(expr.without_parentheses(), Expression::StringLiteral(s) if s.value.as_str().is_empty())
}

/// A short human description of an expression, for hole markers / provenance.
fn describe_expr(expr: &Expression) -> String {
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

// ---------------------------------------------------------------------------
// predicates (`if` test -> when clause)
// ---------------------------------------------------------------------------

struct Predicate {
    object: Option<String>,
    field: String,
    op: String,
    value: serde_yaml::Value,
}

impl Predicate {
    fn when_value(&self) -> serde_yaml::Value {
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

fn extract_predicate(expr: &Expression) -> Option<Predicate> {
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
fn field_of(expr: &Expression) -> Option<(String, Option<String>)> {
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

fn map_binop(op: BinaryOperator) -> Option<&'static str> {
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

fn flip_op(op: &str) -> &str {
    match op {
        "lt" => "gt",
        "lte" => "gte",
        "gt" => "lt",
        "gte" => "lte",
        other => other,
    }
}

fn literal_to_yaml(expr: &Expression) -> Option<serde_yaml::Value> {
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

// ---------------------------------------------------------------------------
// max_iter resolution
// ---------------------------------------------------------------------------

enum MaxIter {
    Value(i64),
    Unresolved,
}

fn resolve_for_bound(test: Option<&Expression>, consts: &Consts) -> MaxIter {
    resolve_test_bound(test, consts)
}

fn resolve_test_bound(test: Option<&Expression>, consts: &Consts) -> MaxIter {
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

fn resolve_number(expr: &Expression, consts: &Consts) -> Option<f64> {
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

// ---------------------------------------------------------------------------
// schema object -> frontmatter
// ---------------------------------------------------------------------------

fn object_to_frontmatter(obj: &ObjectExpression) -> HashMap<String, FrontmatterFieldDecl> {
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

fn map_json_type(ty: &str) -> String {
    match ty {
        "integer" | "number" => "int",
        "boolean" => "bool",
        "array" => "list",
        "string" => "string",
        other => other,
    }
    .to_string()
}

// ---------------------------------------------------------------------------
// small AST helpers
// ---------------------------------------------------------------------------

/// Look up a property by static name on an object literal expression.
fn object_prop<'a>(expr: &'a Expression<'a>, key: &str) -> Option<&'a Expression<'a>> {
    let Expression::ObjectExpression(obj) = expr.without_parentheses() else {
        return None;
    };
    object_prop_obj(obj, key)
}

fn object_prop_obj<'a>(obj: &'a ObjectExpression<'a>, key: &str) -> Option<&'a Expression<'a>> {
    for p in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(op) = p {
            if op.key.static_name().as_deref() == Some(key) {
                return Some(&op.value);
            }
        }
    }
    None
}

fn string_literal_value(expr: &Expression) -> Option<String> {
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

fn string_array_values(expr: &Expression) -> Option<Vec<String>> {
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
fn static_prefix_of(expr: &Expression) -> Option<String> {
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
fn clean_name(s: &str) -> String {
    s.trim_end_matches(['#', '·', '-', ' ', ':', '_'])
        .trim()
        .to_string()
}

fn binding_name<'a>(id: &'a oxc_ast::ast::BindingPattern<'a>) -> Option<&'a str> {
    match id {
        oxc_ast::ast::BindingPattern::BindingIdentifier(b) => Some(b.name.as_str()),
        _ => None,
    }
}

/// Strip parentheses and `await` wrappers.
fn unwrap_expr<'a>(mut expr: &'a Expression<'a>) -> &'a Expression<'a> {
    loop {
        match expr {
            Expression::ParenthesizedExpression(p) => expr = &p.expression,
            Expression::AwaitExpression(a) => expr = &a.argument,
            _ => break,
        }
    }
    expr
}

/// If `expr` is (or wraps, via `.then()` chains) a call to `agent(...)`, return
/// the agent call.
fn as_agent_call<'a>(expr: &'a Expression<'a>) -> Option<&'a CallExpression<'a>> {
    let e = unwrap_expr(expr);
    if let Expression::CallExpression(c) = e {
        if c.callee.is_specific_id("agent") {
            return Some(c);
        }
        // agent(...).then(...) / .catch(...)
        if let Expression::StaticMemberExpression(m) = c.callee.without_parentheses() {
            return as_agent_call(&m.object);
        }
    }
    None
}

fn as_named_call<'a>(expr: &'a Expression<'a>, name: &str) -> Option<&'a CallExpression<'a>> {
    let e = unwrap_expr(expr);
    if let Expression::CallExpression(c) = e {
        if c.callee.is_specific_id(name) {
            return Some(c);
        }
    }
    None
}

// --- deep agent scan (for loop guard, pipeline/parallel stages) ------------

fn stmt_has_agent(stmt: &Statement, depth: u32) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    match stmt {
        Statement::ExpressionStatement(es) => expr_has_agent(&es.expression, depth + 1),
        Statement::VariableDeclaration(d) => d
            .declarations
            .iter()
            .any(|dc| dc.init.as_ref().is_some_and(|e| expr_has_agent(e, depth + 1))),
        Statement::BlockStatement(b) => b.body.iter().any(|s| stmt_has_agent(s, depth + 1)),
        Statement::IfStatement(i) => {
            stmt_has_agent(&i.consequent, depth + 1)
                || i.alternate
                    .as_ref()
                    .is_some_and(|a| stmt_has_agent(a, depth + 1))
        }
        Statement::ForStatement(f) => stmt_has_agent(&f.body, depth + 1),
        Statement::WhileStatement(w) => stmt_has_agent(&w.body, depth + 1),
        Statement::DoWhileStatement(w) => stmt_has_agent(&w.body, depth + 1),
        Statement::TryStatement(t) => {
            t.block.body.iter().any(|s| stmt_has_agent(s, depth + 1))
                || t.finalizer
                    .as_ref()
                    .is_some_and(|f| f.body.iter().any(|s| stmt_has_agent(s, depth + 1)))
        }
        Statement::ReturnStatement(r) => r
            .argument
            .as_ref()
            .is_some_and(|e| expr_has_agent(e, depth + 1)),
        _ => false,
    }
}

fn if_has_agent(i: &oxc_ast::ast::IfStatement) -> bool {
    stmt_has_agent(&i.consequent, 0)
        || i.alternate.as_ref().is_some_and(|a| stmt_has_agent(a, 0))
}

fn expr_has_agent(expr: &Expression, depth: u32) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    let mut found = Vec::new();
    collect_agents_in_expr(expr, &mut found, depth);
    !found.is_empty()
}

/// Collect every `agent(...)` call reachable from `expr` (in source order),
/// without descending into an agent call's own arguments.
fn collect_agents_in_expr<'a>(
    expr: &'a Expression<'a>,
    out: &mut Vec<&'a CallExpression<'a>>,
    depth: u32,
) {
    if depth > MAX_DEPTH {
        return;
    }
    match expr.without_parentheses() {
        Expression::AwaitExpression(a) => collect_agents_in_expr(&a.argument, out, depth + 1),
        Expression::CallExpression(c) => {
            if c.callee.is_specific_id("agent") {
                out.push(c);
                return;
            }
            // Recurse into callee (for `agent(...).then(...)`) and arguments.
            collect_agents_in_expr(&c.callee, out, depth + 1);
            for a in &c.arguments {
                if let Some(e) = a.as_expression() {
                    collect_agents_in_expr(e, out, depth + 1);
                }
            }
        }
        Expression::StaticMemberExpression(m) => collect_agents_in_expr(&m.object, out, depth + 1),
        Expression::ComputedMemberExpression(m) => {
            collect_agents_in_expr(&m.object, out, depth + 1)
        }
        Expression::ArrowFunctionExpression(f) => {
            for s in &f.body.statements {
                collect_agents_in_stmt(s, out, depth + 1);
            }
        }
        Expression::FunctionExpression(f) => {
            if let Some(b) = &f.body {
                for s in &b.statements {
                    collect_agents_in_stmt(s, out, depth + 1);
                }
            }
        }
        Expression::ArrayExpression(arr) => {
            for el in &arr.elements {
                if let Some(e) = el.as_expression() {
                    collect_agents_in_expr(e, out, depth + 1);
                }
            }
        }
        Expression::BinaryExpression(b) => {
            collect_agents_in_expr(&b.left, out, depth + 1);
            collect_agents_in_expr(&b.right, out, depth + 1);
        }
        Expression::LogicalExpression(l) => {
            collect_agents_in_expr(&l.left, out, depth + 1);
            collect_agents_in_expr(&l.right, out, depth + 1);
        }
        Expression::ConditionalExpression(c) => {
            collect_agents_in_expr(&c.test, out, depth + 1);
            collect_agents_in_expr(&c.consequent, out, depth + 1);
            collect_agents_in_expr(&c.alternate, out, depth + 1);
        }
        _ => {}
    }
}

fn collect_agents_in_stmt<'a>(
    stmt: &'a Statement<'a>,
    out: &mut Vec<&'a CallExpression<'a>>,
    depth: u32,
) {
    if depth > MAX_DEPTH {
        return;
    }
    match stmt {
        Statement::ExpressionStatement(es) => collect_agents_in_expr(&es.expression, out, depth + 1),
        Statement::ReturnStatement(r) => {
            if let Some(e) = &r.argument {
                collect_agents_in_expr(e, out, depth + 1);
            }
        }
        Statement::VariableDeclaration(d) => {
            for dc in &d.declarations {
                if let Some(e) = &dc.init {
                    collect_agents_in_expr(e, out, depth + 1);
                }
            }
        }
        Statement::BlockStatement(b) => {
            for s in &b.body {
                collect_agents_in_stmt(s, out, depth + 1);
            }
        }
        Statement::IfStatement(i) => {
            collect_agents_in_stmt(&i.consequent, out, depth + 1);
            if let Some(a) = &i.alternate {
                collect_agents_in_stmt(a, out, depth + 1);
            }
        }
        _ => {}
    }
}

// --- if-guard exit detection ----------------------------------------------

#[derive(PartialEq, Eq)]
enum GuardExit {
    Return,
    Break,
    None,
}

/// Does the `if` consequent guard a `return` or a `break`? Scans the direct
/// statements of the consequent (a block or a single statement).
fn guarded_exit(consequent: &Statement) -> GuardExit {
    fn scan(stmt: &Statement, depth: u32) -> GuardExit {
        if depth > MAX_DEPTH {
            return GuardExit::None;
        }
        match stmt {
            Statement::ReturnStatement(_) => GuardExit::Return,
            Statement::BreakStatement(_) => GuardExit::Break,
            Statement::BlockStatement(b) => {
                for s in &b.body {
                    let e = scan(s, depth + 1);
                    if e != GuardExit::None {
                        return e;
                    }
                }
                GuardExit::None
            }
            _ => GuardExit::None,
        }
    }
    scan(consequent, 0)
}

fn expr_mentions_ident(expr: &Expression, ident: &str, depth: u32) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    match expr.without_parentheses() {
        Expression::Identifier(id) => id.name.as_str() == ident,
        Expression::StaticMemberExpression(m) => expr_mentions_ident(&m.object, ident, depth + 1),
        Expression::ComputedMemberExpression(m) => {
            expr_mentions_ident(&m.object, ident, depth + 1)
                || expr_mentions_ident(&m.expression, ident, depth + 1)
        }
        Expression::BinaryExpression(b) => {
            expr_mentions_ident(&b.left, ident, depth + 1)
                || expr_mentions_ident(&b.right, ident, depth + 1)
        }
        Expression::LogicalExpression(l) => {
            expr_mentions_ident(&l.left, ident, depth + 1)
                || expr_mentions_ident(&l.right, ident, depth + 1)
        }
        Expression::ConditionalExpression(c) => {
            expr_mentions_ident(&c.test, ident, depth + 1)
                || expr_mentions_ident(&c.consequent, ident, depth + 1)
                || expr_mentions_ident(&c.alternate, ident, depth + 1)
        }
        Expression::UnaryExpression(u) => expr_mentions_ident(&u.argument, ident, depth + 1),
        Expression::CallExpression(c) => {
            expr_mentions_ident(&c.callee, ident, depth + 1)
                || c.arguments
                    .iter()
                    .filter_map(|a| a.as_expression())
                    .any(|e| expr_mentions_ident(e, ident, depth + 1))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{self, LoopKind, NodeType};

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

    // --- round-trip on the real simple-bugfix fixture --------------------

    #[test]
    fn round_trip_simple_bugfix_holds_invariants() {
        let src = include_str!("../../../.claude/workflows/simple-bugfix.js");
        let (result, parsed) = import_and_parse(src, "simple-bugfix");

        // Exactly one Start + one End.
        assert_eq!(
            parsed
                .nodes
                .iter()
                .filter(|n| n.node_type == NodeType::Start)
                .count(),
            1
        );
        assert_eq!(
            parsed
                .nodes
                .iter()
                .filter(|n| n.node_type == NodeType::End)
                .count(),
            1
        );

        // A bounded loop, max_iter 5, whose members are the two fix/test nodes.
        let bounded: Vec<_> = parsed
            .loops
            .iter()
            .filter(|l| l.kind == LoopKind::Bounded)
            .collect();
        assert_eq!(bounded.len(), 1, "one bounded region expected");
        let region = bounded[0];
        assert_eq!(
            region.max_iter,
            Some(serde_yaml::Value::Number(serde_yaml::Number::from(5))),
            "max_iter resolved from MAX_ITER = 5"
        );
        assert_eq!(region.members.len(), 2, "fix + test are the loop body");
        // Members are the implementer + tester node ids.
        let impl_id = &parsed.nodes.iter().find(|n| n.name == "implementer").unwrap().id;
        let test_id = &parsed.nodes.iter().find(|n| n.name == "tester").unwrap().id;
        assert!(region.members.contains(impl_id));
        assert!(region.members.contains(test_id));

        // At least one guarded edge (the `verdict !== 'Bug'` exit).
        let whens = when_strings(&parsed);
        assert!(!whens.is_empty(), "at least one when: edge");
        assert!(
            whens.iter().any(|w| w.contains("verdict")),
            "a when: on the triage verdict"
        );

        // Static prompts extracted verbatim.
        assert!(
            prompt_of(&result, &parsed, "tester").contains("Build le projet"),
            "tester prompt kept verbatim"
        );
        assert!(
            prompt_of(&result, &parsed, "ship-it").contains("Commit sur la BRANCHE COURANTE"),
            "ship-it prompt kept verbatim"
        );

        // Interpolated prompt: static text kept AND an annotated hole marker — not
        // a blank placeholder (that would throw away text the runtime uses verbatim).
        let implementer = prompt_of(&result, &parsed, "implementer");
        assert!(implementer.contains("Implemente"), "static text kept");
        assert!(
            implementer.contains("⟨input:"),
            "interpolation rendered as an annotated marker"
        );

        // The debugger's TRIAGE_SCHEMA becomes output-port frontmatter.
        let debugger = parsed.nodes.iter().find(|n| n.name == "debugger").unwrap();
        let fm = debugger.outputs[0]
            .frontmatter
            .as_ref()
            .expect("schema -> frontmatter");
        let verdict = fm.get("verdict").expect("verdict field");
        assert_eq!(verdict.field_type, "enum");
        assert!(verdict
            .allowed
            .as_ref()
            .unwrap()
            .contains(&"Bug".to_string()));
    }

    // --- targeted units ---------------------------------------------------

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

    #[test]
    fn if_equality_becomes_when_edge() {
        let src = r#"const r = await agent(`triage`, { label: 'triage' }); if (r.verdict === 'Bug') { return {} }"#;
        let (_result, parsed) = import_and_parse(src, "t");
        let whens = when_strings(&parsed);
        assert!(
            whens.iter().any(|w| w.contains("verdict") && w.contains("eq") && w.contains("Bug")),
            "if (r.verdict === 'Bug') -> when {{ verdict: {{ eq: Bug }} }}, got {whens:?}"
        );
    }

    #[test]
    fn bare_identifier_equality_maps_field_to_itself() {
        let src = r#"agent(`a`, { label: 'a' }); if (status === 'done') { return {} }"#;
        let (_result, parsed) = import_and_parse(src, "t");
        let whens = when_strings(&parsed);
        assert!(
            whens.iter().any(|w| w.contains("status") && w.contains("done")),
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

    // --- degraded fixture: sandcastle-tdd (mostly placeholders) -----------

    #[test]
    fn sandcastle_tdd_imports_without_panic() {
        let src = include_str!("../../../.claude/workflows/sandcastle-tdd.js");
        let result = import_workflow_js(src, "sandcastle-tdd")
            .expect("even a mostly-placeholder workflow must import without crashing");
        // It must produce a parseable draft…
        pipeline::parse_pipeline(&result.yaml_text)
            .expect("degraded import must still parse");
        // …and flag the lossy translation (helper-nu prompts, nested loops, etc.).
        assert!(
            !result.warnings.is_empty(),
            "the degraded case must raise translation warnings"
        );
        assert_eq!(result.name, "sandcastle-tdd");
    }
}
