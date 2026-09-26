// The fold between an editor node's per-harness `harnesses` map and the single
// `model`/`effort` view the pickers edit (#550, ADR-0046).

import type { HarnessSettings } from "../../shared/generated/HarnessSettings";
import { resolveEditorHarness } from "./catalog";

/** The harness fields of a node, as the editor holds them: the YAML's pin and
 *  per-harness map, plus the flat `model`/`effort` the pickers edit. */
export interface NodeHarnessFields {
  pin_harness?: string | null;
  harnesses?: { [key in string]: HarnessSettings };
  model?: string | null;
  effort?: string | null;
}

/** Load fold: project the RESOLVED harness's `{model, effort}` out of the node's
 *  `harnesses` map onto the flat `model`/`effort` the pickers edit. Returns a new
 *  node; the `harnesses` map is preserved so non-resolved entries survive a
 *  round-trip. Idempotent. */
export function foldHarnessOntoNode<T extends NodeHarnessFields>(node: T): T {
  const resolved = resolveEditorHarness(node);
  const entry = node.harnesses?.[resolved];
  return {
    ...node,
    model: entry?.model ?? null,
    effort: entry?.effort ?? null,
  };
}

/** Save fold: merge the flat `model`/`effort` back into `harnesses[resolved]`,
 *  preserving every other harness's entry, and dropping an entry that carries
 *  neither. Returns the harnesses map to emit (or `undefined` when empty). */
export function foldNodeIntoHarnesses(
  node: NodeHarnessFields,
): Record<string, { model?: string; effort?: string }> | undefined {
  const resolved = resolveEditorHarness(node);
  const out: Record<string, { model?: string; effort?: string }> = {};
  for (const [name, entry] of Object.entries(node.harnesses ?? {})) {
    const model = entry?.model || undefined;
    const effort = entry?.effort || undefined;
    if (model || effort) out[name] = { ...(model ? { model } : {}), ...(effort ? { effort } : {}) };
  }
  const model = node.model || undefined;
  const effort = node.effort || undefined;
  if (model || effort) {
    out[resolved] = { ...(model ? { model } : {}), ...(effort ? { effort } : {}) };
  } else {
    delete out[resolved];
  }
  return Object.keys(out).length > 0 ? out : undefined;
}
