/**
 * Canonical YAML emission for canvas-authored artefacts (#360, lifted verbatim
 * out of `stores/editStore.ts`). Pure: no store, no React, no network — the only
 * dependency is `../types`.
 *
 * Two emitters share one hand-rolled dumper (`yamlDump.ts`):
 *  - `pipelineToYamlObject` / `serializePipeline` — the bytes written to
 *    `.pdo/pipelines/<id>.yaml`, and both sides of the library synced/diverged
 *    comparison (see `hooks/useLibraryPipelines`).
 *  - `exportNodeAsYaml` (#345, `exportNodeYaml.ts`) — the node-library entry shape.
 *
 * CROSS-LANGUAGE CONTRACT, enforced by nothing. Everything this module emits is
 * parsed by the daemon's serde-derived reader (`pipeline::parse_pipeline`, key
 * allow-list `KNOWN_TOP_LEVEL_KEYS` in `crates/pdo-pipeline/src/domain/pipeline/`), and
 * `exportNodeAsYaml`'s output must match `pdo_pipeline::domain::library::LibraryEntry`
 * field-for-field including its omit-when-default policy. No test crosses that
 * boundary: `tests/serializer_round_trip.rs` exercises the daemon's own emitter,
 * not this one. #352 is what the gap costs — a dropped `max_iter` made the daemon
 * reject the save and nothing persisted. Change an emit rule here only against
 * the Rust reader.
 *
 * The two per-port emitters are DELIBERATELY divergent — do not unify them, see
 * the comment on `portToYamlObject` (`exportNodeYaml.ts`).
 */
import type { FrontmatterFieldDecl } from "../shared/generated/FrontmatterFieldDecl";
// #550: the per-harness fold (pure, `../types`-only) — keeps this module pure.
import { foldNodeIntoHarnesses } from "../harness/model/nodeHarness";
import { nodeIsolation } from "./nodeIsolation";
import { hasProvisioningRules } from "../workspace/model/provisioning";
import type { EditorPipelineDef } from "../pipeline/model/pipeline";
import { yamlStringify } from "./yamlDump";

/**
 * Drops the null-valued keys of every frontmatter declaration (#457).
 *
 * The daemon's `FrontmatterFieldDecl.allowed` is an `Option<Vec<String>>` with no
 * `skip_serializing_if`, so `GET /pipelines/...` ships `allowed: null` on every
 * non-enum field. Copying the declaration verbatim carried that null into the
 * YAML, so the FIRST save after a page load rewrote `{type: bool}` as
 * `{allowed: null, type: bool}` — stable afterwards, since the reloaded pipeline
 * then already held the null. A parasitic diff on a git-versioned pipeline, and
 * twin drift for the Library (which stores YAML byte-for-byte).
 *
 * Absent and null must produce the same bytes, so this normalises toward absent:
 * on the reader side serde maps both to `None`. `[]` is NOT null and survives —
 * an enum with an empty allow-list is a statement, not a default.
 *
 * Every one of the three frontmatter emit sites goes through here. Add a fourth
 * and it must too.
 */
export function frontmatterToYamlObject(
  frontmatter: Record<string, FrontmatterFieldDecl>,
): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const [field, decl] of Object.entries(frontmatter)) {
    const cleaned: Record<string, unknown> = {};
    for (const [key, value] of Object.entries(decl)) {
      if (value !== null && value !== undefined) cleaned[key] = value;
    }
    out[field] = cleaned;
  }
  return out;
}

// Canonical plain-object form of a pipeline — the exact structure that gets
// YAML-serialized on save. Also used for semantic comparison against library
// entries (see useLibraryPipelines): building both sides through this single
// code path erases formatting noise (key order, quoting, parser defaults)
// that a textual YAML comparison would misread as divergence.
export function pipelineToYamlObject(p: EditorPipelineDef): Record<string, unknown> {
  const obj: Record<string, unknown> = {
    name: p.name,
  };
  if (p.version) obj.version = p.version;
  // Prompt-optional pipelines (#158) carry an explicit `prompt_required: false`.
  // The default (prompt required) is omitted so the common case stays clean and
  // round-trips by absence — same convention as `loops` and `version`.
  if (p.prompt_required === false) obj.prompt_required = false;
  // #877 / ADR-0076: the pipeline's own wiring-grid size, emitted only when the
  // pipeline chose one — absent, it follows the reader's global default, and an
  // untouched pipeline round-trips byte for byte. Layout (`layoutFields.ts`).
  if (p.grid_size) obj.grid_size = p.grid_size;
  // ADR-0086: « Attente des nœuds lancés », emitted only when on — a pipeline
  // without it serializes byte-identically. Semantic (`layoutFields.ts`).
  if (p.wait_for_started_nodes) obj.wait_for_started_nodes = true;
  if (Object.keys(p.variables).length > 0) {
    const vars: Record<string, unknown> = {};
    // Short form (`name: <default>`) when the daemon would infer the declared
    // type back from the default; the explicit `{ type, default }` form
    // otherwise, so a `float` defaulting to `3` does not reload as an `int`.
    for (const [k, v] of Object.entries(p.variables)) {
      vars[k] = inferVariableType(v.default) === v.type ? v.default : { type: v.type, default: v.default };
    }
    obj.variables = vars;
  }
  obj.nodes = p.nodes.map((n) => {
    const node: Record<string, unknown> = {
      id: n.id,
      name: n.name ?? n.id,
      type: n.type,
    };
    if (n.interactive) node.interactive = true;
    // #653/ADR-0060: where the node works, written UNCONDITIONALLY for an
    // `agent`/`script` — even at the editor default. A document that omits the
    // line makes the reader recall a default; one that states it does not. Never
    // emitted for a structural node.
    const isolation = nodeIsolation(n);
    if (isolation !== null) node.isolated_worktree = isolation;
    // #723/ADR-0064: the « Orchestrator » toggle. Emitted only when on, like
    // `interactive` — a node without the toggle serializes byte-identically to
    // its pre-#723 shape. Must be mirrored in `exportNodeAsYaml` below and
    // classified in `layoutFields.ts` (SEMANTIC_FIELDS.node).
    if (n.orchestrator) node.orchestrator = true;
    // ADR-0086: the « on demand » option, emitted only when on, like
    // `orchestrator`. Mirrored in `exportNodeAsYaml`, SEMANTIC in `layoutFields.ts`.
    if (n.on_demand) node.on_demand = true;
    // The node's own auto-fail override (true or false), emitted only when set:
    // absent means "inherit". Dropping it here lost it on every editor save.
    if (n.auto_fail !== undefined && n.auto_fail !== null) node.auto_fail = n.auto_fail;
    // #550/ADR-0046: the harness axis replaces flat `model:`/`effort:`. The pin is
    // emitted when set; the flat model/effort view is folded back into the
    // resolved harness's entry in the per-harness `harnesses` map, emitted only
    // when non-empty — so an unset node and a library twin with no settings both
    // produce objects without the keys and stay `synced`, not `diverged`. This
    // emitter and `exportNodeAsYaml` are deliberately NOT unified (see the note
    // further down) — a field added here must be added there too.
    if (n.pin_harness) node.pin_harness = n.pin_harness;
    if (n.agent_choice && n.agent_choice.mode !== "inherit") {
      node.agent_choice = n.agent_choice;
    }
    // #669/ADR-0062: the node's skills, by id with their label, emitted only when
    // non-empty so an unset node stays byte-identical to its pre-#669 document.
    if (n.skills && n.skills.length > 0) {
      node.skills = n.skills.map((skill) => ({ id: skill.id, name: skill.name }));
    }
    const harnesses = foldNodeIntoHarnesses(n);
    if (harnesses) node.harnesses = harnesses;
    if (n.provisioning && hasProvisioningRules(n.provisioning)) {
      node.provisioning = n.provisioning;
    }
    // Legacy `type: loop` nodes (pre-region model, ADR-0011) carry a node-level
    // `max_iter` that the daemon still requires and validates
    // (`pipeline.rs` `NodeType::Loop`). The current model emits `max_iter` on the
    // `loops:` region below, not on any node — but a legacy loop node has no
    // matching region, so if its bound isn't round-tripped here the daemon
    // rejects the save with "loop node '<id>' must declare 'max_iter'" and
    // nothing persists (#352). Bounded loops are the only nodes that carry
    // `node.max_iter` (regular nodes never set it), so its presence is the
    // signal — this mirrors the region emit and keeps non-loop nodes clean.
    if (n.max_iter !== undefined && n.max_iter !== null) node.max_iter = n.max_iter;
    // A collection's `over` driver now lives on the `loops:` region, not on any
    // node (#151) — no node-level `over` serialization.
    if (n.inputs.length > 0)
      node.inputs = n.inputs.map((port) => {
        const p: Record<string, unknown> = { name: port.name };
        if (port.repeated) p.repeated = true;
        // A port the completion refuses to leave empty. Emitted only when set,
        // like `repeated`: dropping it here lost it on every editor save.
        if (port.required) p.required = true;
        if (port.side) p.side = port.side;
        if (port.port_type && port.port_type !== "markdown")
          p.port_type = port.port_type;
        if (port.frontmatter) p.frontmatter = frontmatterToYamlObject(port.frontmatter);
        return p;
      });
    if (n.outputs.length > 0)
      node.outputs = n.outputs.map((port) => {
        const p: Record<string, unknown> = { name: port.name };
        if (port.repeated) p.repeated = true;
        // A port the completion refuses to leave empty. Emitted only when set,
        // like `repeated`: dropping it here lost it on every editor save.
        if (port.required) p.required = true;
        if (port.side) p.side = port.side;
        if (port.port_type && port.port_type !== "markdown")
          p.port_type = port.port_type;
        if (port.frontmatter) p.frontmatter = frontmatterToYamlObject(port.frontmatter);
        if (port.when) p.when = port.when;
        if (port.instructions?.trim()) p.instructions = port.instructions;
        return p;
      });
    if (n.view) node.view = n.view;
    return node;
  });
  obj.edges = p.edges.map((e) => {
    const edge: Record<string, unknown> = {
      source: e.source,
      target: e.target,
    };
    // Conditional routing (ADR-0011): a guarded edge carries `when:`, a
    // fallback edge carries `else: true`. Both live on the edge now, not on a
    // Switch node's output ports.
    if (e.when && Object.keys(e.when).length > 0) edge.when = e.when;
    if (e.else === true) edge.else = true;
    // Routing (#154): only manually-pinned edges persist their route. Auto
    // edges recompute deterministically, so they store no `mode`/`waypoints` —
    // emitting them would be noise. A `manual` mode without waypoints is also
    // meaningless (nothing pinned), so guard on a non-empty waypoint list.
    if (e.mode === "manual" && e.waypoints && e.waypoints.length > 0) {
      edge.mode = "manual";
      edge.waypoints = e.waypoints.map((w) => ({ x: w.x, y: w.y }));
    }
    // Drop-position anchor side (#168). Layout, like mode/waypoints: persists so
    // a shared workflow keeps its arrow arrival sides. `left` is the legacy
    // default and round-trips by absence, so emit only the other three sides.
    if (e.target_side && e.target_side !== "left") {
      edge.target_side = e.target_side;
    }
    // Per-edge anchors (#844). Layout, like the three above: they say WHERE on a
    // card's border the wire leaves and lands, so a shared workflow keeps the
    // wiring it was drawn with. Absent ⇒ the side's middle (the pre-#844
    // geometry), so an untouched edge round-trips byte for byte. The offset is
    // rounded on persist — sub-pixel handle centres are what turn a straight wire
    // into a three-segment path with two phantom handles.
    if (e.source_anchor) {
      edge.source_anchor = {
        side: e.source_anchor.side,
        offset: Math.round(e.source_anchor.offset),
      };
    }
    if (e.target_anchor) {
      edge.target_anchor = {
        side: e.target_anchor.side,
        offset: Math.round(e.target_anchor.offset),
      };
    }
    // Labels and draw order (#845). Layout again, and again emitted only when
    // NON-DEFAULT, so a pipeline nobody re-decorated saves back byte-identical:
    //  - `show_output_labels` has no constant default (it is derived from the
    //    source node's declared output count), so absence means "follow the
    //    derivation" and any explicit value — `false` included — is emitted;
    //  - an empty `output_label_pos` map is the same as no map at all;
    //  - `below_nodes: false` is the default draw order, above the cards.
    if (e.show_output_labels != null) edge.show_output_labels = e.show_output_labels;
    if (e.output_label_pos && Object.keys(e.output_label_pos).length > 0) {
      edge.output_label_pos = Object.fromEntries(
        Object.entries(e.output_label_pos).map(([port, p]) => [port, { x: p.x, y: p.y }]),
      );
    }
    if (e.condition_label_pos) {
      edge.condition_label_pos = { x: e.condition_label_pos.x, y: e.condition_label_pos.y };
    }
    if (e.below_nodes === true) edge.below_nodes = true;
    return edge;
  });

  // Named bounded loop regions (ADR-0011 / #148). Emitted only when present so
  // loop-less pipelines stay clean and round-trip identically.
  if (p.loops && p.loops.length > 0) {
    obj.loops = p.loops.map((r) => {
      const region: Record<string, unknown> = {
        id: r.id,
        kind: r.kind,
        members: r.members,
      };
      if (r.max_iter !== undefined && r.max_iter !== null)
        region.max_iter = r.max_iter;
      // A collection region's iterated field (#151 / #269) — without it, the
      // region round-trips as an over-less shell the daemon can't fan out.
      if (r.over) region.over = r.over;
      return region;
    });
  }

  // Inert canvas notes (#307 / ADR-0018) — a top-level `notes:` block, sibling
  // of `loops:`. Emitted only when present so note-less pipelines stay clean and
  // round-trip identically. This is the emit half of the emit/strip couple: the
  // strip (`comparablePipelineObject`) keeps notes out of the semantic diff.
  if (p.notes && p.notes.length > 0) {
    obj.notes = p.notes.map((n) => {
      const note: Record<string, unknown> = { id: n.id, content: n.content };
      if (n.view) note.view = { x: n.view.x, y: n.view.y };
      return note;
    });
  }

  return obj;
}

export function serializePipeline(p: EditorPipelineDef): string {
  return yamlStringify(pipelineToYamlObject(p));
}

/**
 * The type the daemon infers from a short-form variable's value
 * (`infer_variable_type`, `pdo-pipeline` `domain/pipeline/definition.rs`): bool,
 * int / float by the number's form, list for a sequence, string for anything
 * else. A JS number with no fractional part serializes as an integer, hence `int`.
 */
function inferVariableType(value: unknown): string {
  if (typeof value === "boolean") return "bool";
  if (typeof value === "number") return Number.isInteger(value) ? "int" : "float";
  if (Array.isArray(value)) return "list";
  return "string";
}
