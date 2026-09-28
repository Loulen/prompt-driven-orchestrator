/**
 * The node-library YAML of one node (#345): the map a `~/.pdo/library/*.yaml`
 * file carries, emitted field-for-field like `pdo_pipeline::domain::library::
 * LibraryEntry` reads it (see the CROSS-LANGUAGE CONTRACT in
 * `serializePipeline.ts`).
 */
import type { PortDef } from "../shared/generated/PortDef";
import type { PortSide } from "../shared/generated/PortSide";
import { foldNodeIntoHarnesses } from "../harness/model/nodeHarness";
import { nodeIsolation } from "./nodeIsolation";
import { hasProvisioningRules } from "../workspace/model/provisioning";
import type { EditorNodeDef } from "../pipeline/model/pipeline";
import { frontmatterToYamlObject } from "./serializePipeline";
import { yamlStringify } from "./yamlDump";

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
  if (port.required) p.required = true;
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
export function exportNodeAsYaml(node: EditorNodeDef, prompt: string): string {
  const obj: Record<string, unknown> = {
    name: node.name ?? "",
    type: node.type,
  };
  if (node.interactive) obj.interactive = true;
  // #653: same unconditional emit as `pipelineToYamlObject` — an exported node
  // carries its worktree placement, so a re-import does not re-guess it.
  const isolation = nodeIsolation(node);
  if (isolation !== null) obj.isolated_worktree = isolation;
  // #723/ADR-0064: same conditional emit as `pipelineToYamlObject` — the
  // « Orchestrator » toggle travels with an exported node.
  if (node.orchestrator) obj.orchestrator = true;
  if (node.auto_fail !== undefined && node.auto_fail !== null) obj.auto_fail = node.auto_fail;
  if (node.pin_harness) obj.pin_harness = node.pin_harness;
  if (node.agent_choice && node.agent_choice.mode !== "inherit") {
    obj.agent_choice = node.agent_choice;
  }
  if (node.skills && node.skills.length > 0) {
    obj.skills = node.skills.map((skill) => ({ id: skill.id, name: skill.name }));
  }
  const harnesses = foldNodeIntoHarnesses(node);
  if (harnesses) obj.harnesses = harnesses;
  if (node.provisioning && hasProvisioningRules(node.provisioning)) {
    obj.provisioning = node.provisioning;
  }
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
