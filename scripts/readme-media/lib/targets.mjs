// The target pipelines (ADR-0074 §6, CONTEXT.md « Pipeline cible »): drawn by
// the maintainer in the PDO editor, versioned in fixture/targets/, and installed
// AS IS — byte for byte, only `name:` set to the demo name. Nothing here
// generates or touches a position, an anchor, a route or a label.
//
//   - installTarget / installTargets: put targets in a pipelines library (the
//     demo HOME's, or the scene's swap of `implement-review`);
//   - runSnapshot: the run snapshot (`run_started`'s `node_defs` / `edges`) a
//     daemon would freeze from a target — what the mocked history uses;
//   - exportTargets / importTargets: `make readme-media-export` / `-import`,
//     between the fixture and the maintainer's library (`readme-*` names);
//   - withNodeFlags: a node flag (orchestrator, interactive) turned on in a
//     target's YAML, every other byte kept.
//
// Pure file operations: a directory in, a directory out. Callers pass the
// library directory (the demo HOME's, or the user's for export/import).

import fs from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const require = createRequire(path.resolve(here, "..", "..", "..", "frontend", "package.json"));
const yamlLib = require("js-yaml");

export const TARGETS_DIR = path.resolve(here, "..", "fixture", "targets");

/** The pipeline every live scene runs, and that the mocked history snapshots. */
export const DEMO_PIPELINE_ID = "implement-review";

/**
 * The three targets. `name` is the demo name (the `name:` installed in the demo
 * instance, and the library file `<name>.yaml`); `library` the name the export
 * gives it in the maintainer's library. `installed`: part of the demo library
 * from the start (the built `implement-review` shares its name with the
 * complete one, so only the scene that films it swaps it in). The built state
 * has no prompts of its own: it takes `implement-review`'s, for its nodes.
 */
export const TARGETS = [
  { id: "implement-review", file: "implement-review.yaml", name: "implement-review", library: "readme-implement-review", prompts: "implement-review.prompts", installed: true },
  { id: "implement-review.pipelines", file: "implement-review.pipelines.yaml", name: "implement-review", library: "readme-pipelines", prompts: "implement-review.prompts", sharedPrompts: true, installed: false },
  { id: "prod-check", file: "prod-check.yaml", name: "prod-check", library: "prod-check", prompts: "prod-check.prompts", installed: true },
];

export function target(id) {
  const found = TARGETS.find((t) => t.id === id);
  if (!found) throw new Error(`unknown target "${id}" (${TARGETS.map((t) => t.id).join(", ")})`);
  return found;
}

export function parseYaml(text) {
  return yamlLib.load(text);
}

const NAME_LINE = /^name:[^\n]*$/m;

/** `yaml` with its top-level `name:` set to `name`; every other byte kept. */
export function withName(yaml, name) {
  if (!NAME_LINE.test(yaml)) throw new Error("no top-level `name:` line in the pipeline");
  return yaml.replace(NAME_LINE, `name: ${name}`);
}

/** The prompts of a directory: node id → content (`<node>.md`). */
export function readPrompts(dir) {
  const prompts = {};
  if (!fs.existsSync(dir)) return prompts;
  for (const file of fs.readdirSync(dir).sort()) {
    if (file.endsWith(".md")) prompts[file.slice(0, -3)] = fs.readFileSync(path.join(dir, file), "utf8");
  }
  return prompts;
}

/** A target as versioned: its YAML verbatim and its prompts (only those of its
 *  nodes when it shares another target's prompts). */
export function readTarget(id, { targetsDir = TARGETS_DIR } = {}) {
  const t = target(id);
  const yaml = fs.readFileSync(path.join(targetsDir, t.file), "utf8");
  let prompts = readPrompts(path.join(targetsDir, t.prompts));
  if (t.sharedPrompts) {
    const nodes = new Set((parseYaml(yaml).nodes ?? []).map((n) => n.id));
    prompts = Object.fromEntries(Object.entries(prompts).filter(([node]) => nodes.has(node)));
  }
  return { target: t, yaml, prompts };
}

/** A target, parsed. */
export function targetPipeline(id, options) {
  return parseYaml(readTarget(id, options).yaml);
}

/** Write a pipeline and its prompts into a library, as the daemon lays it out:
 *  `<name>.yaml` and `<name>.prompts/<node>.md`, the prompts dir replaced whole. */
export function writeLibraryPipeline(libraryDir, name, { yaml, prompts }) {
  fs.mkdirSync(libraryDir, { recursive: true });
  fs.writeFileSync(path.join(libraryDir, `${name}.yaml`), yaml);
  writePromptsDir(path.join(libraryDir, `${name}.prompts`), prompts);
}

function writePromptsDir(dir, prompts) {
  fs.rmSync(dir, { recursive: true, force: true });
  if (Object.keys(prompts).length === 0) return;
  fs.mkdirSync(dir, { recursive: true });
  for (const [node, content] of Object.entries(prompts)) fs.writeFileSync(path.join(dir, `${node}.md`), content);
}

/** Install one target in a library under its demo name: the YAML byte for byte
 *  but `name:`, and its prompts. Returns the YAML written. */
export function installTarget(libraryDir, id, { targetsDir = TARGETS_DIR, yaml: override } = {}) {
  const { target: t, yaml, prompts } = readTarget(id, { targetsDir });
  const installed = withName(override ?? yaml, t.name);
  writeLibraryPipeline(libraryDir, t.name, { yaml: installed, prompts });
  return installed;
}

/** The demo library as a fresh instance gets it: every `installed` target. */
export function installTargets(libraryDir, options) {
  return TARGETS.filter((t) => t.installed).map((t) => ({ id: t.id, name: t.name, yaml: installTarget(libraryDir, t.id, options) }));
}

// ---- the run snapshot -------------------------------------------------------

const port = (p, defaultSide) => ({ name: p.name, side: p.side ?? defaultSide, ...(p.description ? { description: p.description } : {}) });

/** The edge fields of the #840 model the snapshot carries as drawn (the
 *  daemon's event log reads the first ones; the rest ride along). */
const EDGE_LAYOUT = ["mode", "waypoints", "target_side", "source_anchor", "target_anchor", "output_label_pos", "condition_label_pos"];

/**
 * The run snapshot a daemon freezes from `pipeline` (a parsed target) at run
 * start, as `run_started`'s `node_defs` / `edges`: the daemon's own mapping
 * (`node_def_from_pipeline` / `edge_info_from_pipeline`), plus the edge as the
 * #840 model draws it — every source port, its condition, anchors, route and
 * labels, copied untouched. Pure.
 */
export function runSnapshot(pipeline, { id = pipeline.name } = {}) {
  const nodeDefs = (pipeline.nodes ?? []).map((n) => ({
    id: n.id,
    name: n.name ?? n.id,
    node_type: n.type,
    ...(n.isolated_worktree === undefined ? {} : { isolated_worktree: n.isolated_worktree }),
    ...(n.orchestrator ? { orchestrator: true } : {}),
    view_x: n.view?.x ?? null,
    view_y: n.view?.y ?? null,
    inputs: (n.inputs ?? []).map((p) => port(p, "left")),
    outputs: (n.outputs ?? []).map((p) => port(p, "right")),
  }));
  const edges = (pipeline.edges ?? []).map((e) => {
    const ports = e.source.ports ?? [e.source.port];
    const edge = {
      source_node: e.source.node,
      source_port: ports[0],
      target_node: e.target.node,
      target_port: e.target.port,
      ...(e.reason ? { halt_message: e.reason } : {}),
    };
    if (ports.length > 1) edge.source_ports = ports;
    if (e.when !== undefined) edge.when = e.when;
    for (const key of EDGE_LAYOUT) if (e[key] !== undefined) edge[key] = structuredClone(e[key]);
    return edge;
  });
  return { id, name: pipeline.name, nodeDefs, edges, loops: structuredClone(pipeline.loops ?? []) };
}

// ---- export / import --------------------------------------------------------

/** What the export writes for a target in the library: the YAML named `readme-*`, and its prompts. */
function exported(t, targetsDir) {
  const { yaml, prompts } = readTarget(t.id, { targetsDir });
  return { yaml: withName(yaml, t.library), prompts };
}

function libraryState(libraryDir, name) {
  const file = path.join(libraryDir, `${name}.yaml`);
  if (!fs.existsSync(file)) return null;
  return { yaml: fs.readFileSync(file, "utf8"), prompts: readPrompts(path.join(libraryDir, `${name}.prompts`)) };
}

const samePrompts = (a, b) => JSON.stringify(Object.entries(a).sort()) === JSON.stringify(Object.entries(b).sort());
const samePipeline = (a, b) => a.yaml === b.yaml && samePrompts(a.prompts, b.prompts);

export class ExportConflict extends Error {
  constructor(conflicts) {
    super(
      `refusing to overwrite ${conflicts.map((c) => c.library).join(", ")}: modified in the library since the last export. ` +
        "Bring the change into the fixture with `make readme-media-import`, or delete the pipeline from the library, then export again.",
    );
    this.conflicts = conflicts;
  }
}

/**
 * Copy the targets into `libraryDir` under their `readme-*` names, with their
 * prompts. A pipeline already there and identical is left as is; one that
 * differs was modified since: nothing at all is written, and ExportConflict
 * names them.
 */
export function exportTargets({ libraryDir, targetsDir = TARGETS_DIR }) {
  const plan = TARGETS.map((t) => ({ t, want: exported(t, targetsDir), have: libraryState(libraryDir, t.library) }));
  const conflicts = plan.filter((p) => p.have && !samePipeline(p.have, p.want)).map((p) => ({ id: p.t.id, library: p.t.library }));
  if (conflicts.length > 0) throw new ExportConflict(conflicts);
  const written = [];
  const unchanged = [];
  for (const { t, want, have } of plan) {
    if (have) {
      unchanged.push(t.library);
      continue;
    }
    writeLibraryPipeline(libraryDir, t.library, want);
    written.push(t.library);
  }
  return { written, unchanged };
}

/**
 * The reverse: copy each `readme-*` pipeline of `libraryDir` back into the
 * fixture, `name:` set back to the demo name, prompts included. A target whose
 * pipeline is missing from the library is an error (nothing is written). The
 * built state shares `implement-review`'s prompts: they come from
 * `readme-implement-review`, and a different prompt in `readme-pipelines` is a
 * warning, not an import.
 */
export function importTargets({ libraryDir, targetsDir = TARGETS_DIR }) {
  const plan = TARGETS.map((t) => ({ t, have: libraryState(libraryDir, t.library) }));
  const missing = plan.filter((p) => !p.have).map((p) => p.t.library);
  if (missing.length > 0) {
    throw new Error(`not in the library (${libraryDir}): ${missing.join(", ")}. Run \`make readme-media-export\` first.`);
  }
  const updated = [];
  const unchanged = [];
  const warnings = [];
  const owned = plan.filter((p) => !p.t.sharedPrompts);
  for (const { t, have } of plan) {
    const file = path.join(targetsDir, t.file);
    const yaml = withName(have.yaml, t.name);
    let changed = false;
    if (!fs.existsSync(file) || fs.readFileSync(file, "utf8") !== yaml) {
      fs.writeFileSync(file, yaml);
      changed = true;
    }
    if (t.sharedPrompts) {
      const owner = owned.find((p) => p.t.prompts === t.prompts);
      for (const [node, content] of Object.entries(have.prompts)) {
        if (owner.have.prompts[node] !== content) {
          warnings.push(`${t.library}: the prompt of \`${node}\` differs from ${owner.t.library}'s and was not imported (edit prompts in ${owner.t.library})`);
        }
      }
    } else {
      const dir = path.join(targetsDir, t.prompts);
      if (!samePrompts(readPrompts(dir), have.prompts)) {
        writePromptsDir(dir, have.prompts);
        changed = true;
      }
    }
    (changed ? updated : unchanged).push(t.id);
  }
  return { updated, unchanged, warnings };
}

// ---- node flags ---------------------------------------------------------------

/**
 * `yaml` with flags turned on for one node (`{ orchestrator: true }`,
 * `{ interactive: true }`): each goes on its own line right under the node's
 * `type:`, at the node's indentation. Every other byte is kept, so the layout
 * stays the maintainer's. Throws when the node is not found or already sets
 * the flag.
 */
export function withNodeFlags(yaml, nodeId, flags) {
  const lines = yaml.split("\n");
  const start = lines.findIndex((l) => new RegExp(`^(\\s*)- id: ${escapeRegExp(nodeId)}\\s*$`).test(l));
  if (start < 0) throw new Error(`no node \`${nodeId}\` in the pipeline`);
  const indent = `${lines[start].match(/^(\s*)-/)[1]}  `;
  let typeLine = -1;
  // The node's own keys sit at `indent`; deeper lines belong to them, and the
  // next `- id:` (less indented) ends the node.
  for (let i = start + 1; i < lines.length && lines[i].startsWith(indent); i++) {
    const rest = lines[i].slice(indent.length);
    if (rest.startsWith(" ")) continue;
    const key = rest.match(/^([A-Za-z_]+):/)?.[1];
    if (key && key in flags) throw new Error(`node \`${nodeId}\` already sets \`${key}\``);
    if (key === "type") typeLine = i;
  }
  if (typeLine < 0) throw new Error(`node \`${nodeId}\` has no \`type:\` line`);
  const added = Object.entries(flags).map(([key, value]) => `${indent}${key}: ${value}`);
  lines.splice(typeLine + 1, 0, ...added);
  return lines.join("\n");
}

function escapeRegExp(text) {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
