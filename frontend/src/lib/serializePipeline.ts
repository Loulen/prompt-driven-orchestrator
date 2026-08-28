/**
 * Canonical YAML emission for canvas-authored artefacts (#360, lifted verbatim
 * out of `stores/editStore.ts`). Pure: no store, no React, no network — the only
 * dependency is `../types`.
 *
 * Two emitters share one hand-rolled dumper:
 *  - `pipelineToYamlObject` / `serializePipeline` — the bytes written to
 *    `.pdo/pipelines/<id>.yaml`, and both sides of the library synced/diverged
 *    comparison (see `hooks/useLibraryPipelines`).
 *  - `exportNodeAsYaml` (#345) — the node-library entry shape.
 *
 * CROSS-LANGUAGE CONTRACT, enforced by nothing. Everything this module emits is
 * parsed by the daemon's serde-derived reader (`pipeline::parse_pipeline`, key
 * allow-list `KNOWN_TOP_LEVEL_KEYS` in `crates/pdo-daemon/src/pipeline.rs`), and
 * `exportNodeAsYaml`'s output must match `library_store::LibraryEntry`
 * field-for-field including its omit-when-default policy. No test crosses that
 * boundary: `tests/serializer_round_trip.rs` exercises the daemon's own emitter,
 * not this one. #352 is what the gap costs — a dropped `max_iter` made the daemon
 * reject the save and nothing persisted. Change an emit rule here only against
 * the Rust reader.
 *
 * The two per-port emitters are DELIBERATELY divergent — do not unify them, see
 * the comment on `portToYamlObject`.
 */
import type {
  PipelineDef,
  NodeDef,
  PortDef,
  PortSide,
  FrontmatterFieldDecl,
} from "../types";
// #550: the per-harness fold (pure, `../types`-only) — keeps this module pure.
import { foldNodeIntoHarnesses } from "./harness";

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
function frontmatterToYamlObject(
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
export function pipelineToYamlObject(p: PipelineDef): Record<string, unknown> {
  const obj: Record<string, unknown> = {
    name: p.name,
  };
  if (p.version) obj.version = p.version;
  // Prompt-optional pipelines (#158) carry an explicit `prompt_required: false`.
  // The default (prompt required) is omitted so the common case stays clean and
  // round-trips by absence — same convention as `loops` and `version`.
  if (p.prompt_required === false) obj.prompt_required = false;
  if (Object.keys(p.variables).length > 0) {
    const vars: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(p.variables)) {
      vars[k] = v.default;
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
    const harnesses = foldNodeIntoHarnesses(n);
    if (harnesses) node.harnesses = harnesses;
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

export function serializePipeline(p: PipelineDef): string {
  return yamlStringify(pipelineToYamlObject(p));
}

// Serialize one authored port to the node-library YAML shape (#345), mirroring
// `pipelineToYamlObject`'s per-port emit: `name` always; the rest only when
// non-default so a plain port stays a clean `- name: x`. `side` is omitted when
// it equals the direction's default (left for inputs, right for outputs) — the
// value the daemon/`libraryPortToPortDef` re-fill on import, so it round-trips
// by absence.
// NOT a duplicate of the inline per-port emit in `pipelineToYamlObject` above:
// that one emits `side` whenever set and `when` on outputs only; this one omits
// `side` when it equals the direction default (the value the daemon refills on
// import, so it round-trips by absence) and emits `when` on both directions.
// Unifying them strips `side:` from every pipeline file and flips every library
// star to `diverged`. #355 makes `side` SEMANTIC.
function portToYamlObject(port: PortDef, defaultSide: PortSide): Record<string, unknown> {
  const p: Record<string, unknown> = { name: port.name };
  if (port.repeated) p.repeated = true;
  if (port.side && port.side !== defaultSide) p.side = port.side;
  if (port.port_type && port.port_type !== "markdown") p.port_type = port.port_type;
  if (port.frontmatter) p.frontmatter = frontmatterToYamlObject(port.frontmatter);
  if (port.when) p.when = port.when;
  if (port.instructions?.trim()) p.instructions = port.instructions;
  return p;
}

// Emit a prompt as a YAML block scalar (#345) — the readable, copy-pasteable
// form the issue asks for. NOT via `dumpYaml`: that JSON-escapes any multi-line
// string onto a single illegible line (see the string branch of `dumpYaml`).
//
// An explicit indentation indicator (`|2` / `|2-`) fixes the block indent at 2
// regardless of the prompt's own leading whitespace — without it, a first line
// more-indented than a later one would make YAML end the block early and
// misparse the rest. The chomping indicator preserves the exact trailing
// newline: `|2-` (strip) when the prompt has none, `|2` (clip) keeps exactly
// one when it does — and clip only keeps a newline the source physically ends
// in, so we emit that newline below. An import→export round-trip is thus
// byte-stable. Empty prompt → `prompt: ""` (a block scalar cannot express the
// empty string).
function promptToBlockScalar(prompt: string): string {
  if (prompt === "") return 'prompt: ""';
  const hasTrailingNewline = prompt.endsWith("\n");
  const body = hasTrailingNewline ? prompt.slice(0, -1) : prompt;
  const indicator = hasTrailingNewline ? "|2" : "|2-";
  const indented = body
    .split("\n")
    .map((line) => (line.length > 0 ? `  ${line}` : ""))
    .join("\n");
  // Clip (`|2`) only keeps a trailing newline that physically ends the source,
  // so emit that newline when the prompt has one; strip (`|2-`) needs none.
  // Without this the `\n` is silently dropped and the round-trip diverges (the
  // stripped node then re-exports as `|2-`), so the two indicators are a no-op.
  return `prompt: ${indicator}\n${indented}${hasTrailingNewline ? "\n" : ""}`;
}

/**
 * Serialize a single node to the node-library YAML shape (#345): the same map a
 * `~/.pdo/library/*.yaml` file carries, so the output is directly re-importable
 * via `Add node from YAML…` and an actual library file exports/imports the same
 * way. Structural fields go through `dumpYaml`; the prompt is appended by hand
 * as a block scalar (see `promptToBlockScalar`).
 *
 * NEVER emits `id` (regenerated on add), `view` (re-centred), edges (they live
 * at pipeline level), or `over` (a region driver, not a node field). Legacy
 * `max_iter` (bounded-loop nodes) is emitted only when present.
 *
 * #616 (correctif 7): the harness axis round-trips like the pipeline emitter — the
 * `pin_harness` and the per-harness `harnesses` map, NOT a flat `model:`/`effort:`.
 * Flat keys re-homed the value onto the RESOLVED harness on reimport, which is
 * `claude` for an unpinned node — so a node pinned on another harness silently lost
 * its settings onto claude on the round-trip. Emitting the pin and the map preserves
 * both. This emitter and `pipelineToYamlObject` must stay in lockstep (see the note
 * on that function): a harness field added there must be added here too.
 */
export function exportNodeAsYaml(node: NodeDef, prompt: string): string {
  const obj: Record<string, unknown> = {
    name: node.name ?? "",
    type: node.type,
  };
  if (node.interactive) obj.interactive = true;
  if (node.pin_harness) obj.pin_harness = node.pin_harness;
  if (node.agent_choice && node.agent_choice.mode !== "inherit") {
    obj.agent_choice = node.agent_choice;
  }
  const harnesses = foldNodeIntoHarnesses(node);
  if (harnesses) obj.harnesses = harnesses;
  // Legacy bounded-loop nodes carry a node-level `max_iter` the daemon still
  // requires; regular nodes never set it, so its presence is the signal.
  if (node.max_iter !== undefined && node.max_iter !== null) obj.max_iter = node.max_iter;
  if (node.inputs.length > 0) {
    obj.inputs = node.inputs.map((p) => portToYamlObject(p, "left"));
  }
  if (node.outputs.length > 0) {
    obj.outputs = node.outputs.map((p) => portToYamlObject(p, "right"));
  }
  return `${yamlStringify(obj)}\n${promptToBlockScalar(prompt)}`;
}

function yamlStringify(obj: unknown): string {
  return dumpYaml(obj, 0);
}

function dumpYaml(val: unknown, indent: number): string {
  const prefix = "  ".repeat(indent);
  if (val === null || val === undefined) return "null";
  if (typeof val === "boolean") return val ? "true" : "false";
  if (typeof val === "number") return String(val);
  if (typeof val === "string") {
    if (val.includes("\n") || val.includes(":") || val.includes("#") || val.includes('"') || val === "") {
      return JSON.stringify(val);
    }
    if (/^\d/.test(val) || val === "true" || val === "false" || val === "null") {
      return JSON.stringify(val);
    }
    return val;
  }
  if (Array.isArray(val)) {
    if (val.length === 0) return "[]";
    const isSimple = val.every(
      (v) => typeof v === "string" || typeof v === "number" || typeof v === "boolean",
    );
    if (isSimple) {
      return `[${val.map((v) => dumpYaml(v, 0)).join(", ")}]`;
    }
    return val
      .map((v) => {
        const child = dumpYaml(v, indent + 1);
        if (typeof v === "object" && v !== null && !Array.isArray(v)) {
          const lines = child.split("\n");
          // The recursive call already indented continuation lines at indent+1,
          // which lines up with the column where the first key lands after `- `
          // — so pass them through verbatim.
          const rest = lines.slice(1).join("\n");
          return rest
            ? `${prefix}- ${lines[0]}\n${rest}`
            : `${prefix}- ${lines[0]}`;
        }
        return `${prefix}- ${child}`;
      })
      .join("\n");
  }
  if (typeof val === "object") {
    const entries = Object.entries(val as Record<string, unknown>);
    if (entries.length === 0) return "{}";
    const isFlowable = entries.every(
      ([, v]) => typeof v !== "object" || v === null,
    );
    if (isFlowable && entries.length <= 3) {
      const inner = entries.map(([k, v]) => `${k}: ${dumpYaml(v, 0)}`).join(", ");
      return `{ ${inner} }`;
    }
    return entries
      .map(([k, v]) => {
        const child = dumpYaml(v, indent + 1);
        if (typeof v === "object" && v !== null && !Array.isArray(v)) {
          const lines = child.split("\n");
          if (lines.length === 1 && lines[0].startsWith("{")) {
            return `${k}: ${lines[0]}`;
          }
          const [first, ...rest] = lines;
          const head = `${k}:\n  ${prefix}${first}`;
          return rest.length > 0 ? `${head}\n${rest.join("\n")}` : head;
        }
        if (Array.isArray(v) && v.length > 0 && !v.every((x) => typeof x !== "object" || x === null)) {
          return `${k}:\n${child}`;
        }
        return `${k}: ${child}`;
      })
      .join("\n" + prefix);
  }
  return String(val);
}
