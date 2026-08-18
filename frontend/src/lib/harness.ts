// The agentic-harness axis, client side (#550, ADR-0046).
//
// The daemon owns the truth (descriptor + resolver); this module is the thin
// client mirror the editor needs: which harnesses exist, which one an editor node
// resolves to, whether a harness exposes an effort axis (to grey the picker), and
// the fold between the node's per-harness `harnesses` map and the single
// `model`/`effort` view the existing pickers edit.

import type { NodeDef } from "../types";

/** The floor of the precedence chain — a node with no pin runs on `claude`. */
export const HARNESS_FLOOR = "claude";

/** The harnesses PDO ships embedded (ADR-0045). The pin selector offers these
 *  plus "Default" (follow the tier above). A user-declared disk harness (#553)
 *  would extend this; not in this slice. */
export const KNOWN_HARNESSES = ["claude", "opencode"] as const;

/** Client mirror of the descriptor's `{effort}` hole (ADR-0045): `opencode` has
 *  no launch-time effort axis, so its effort picker is greyed. An unknown harness
 *  defaults to "has effort" — the conservative choice (don't grey what we can't
 *  see; the daemon still ignores an effort a harness can't honour). */
const HARNESS_HAS_EFFORT: Record<string, boolean> = {
  claude: true,
  opencode: false,
};

export function harnessHasEffort(name: string): boolean {
  return HARNESS_HAS_EFFORT[name] ?? true;
}

/** The harness an editor node resolves to, with only the tiers a client knows:
 *  the node's pin, else the `claude` floor. (The instance default is a coarser
 *  tier the editor does not fetch per-node; the floor is the safe editor default,
 *  and the daemon re-resolves authoritatively at spawn.) An empty pin is "unset". */
export function resolveEditorHarness(node: Pick<NodeDef, "pin_harness">): string {
  const pin = node.pin_harness;
  return pin && pin !== "" ? pin : HARNESS_FLOOR;
}

/** Load fold: project the RESOLVED harness's `{model, effort}` out of the node's
 *  `harnesses` map onto the flat `model`/`effort` the pickers edit. Returns a new
 *  node; the `harnesses` map is preserved so non-resolved entries survive a
 *  round-trip. Idempotent. */
export function foldHarnessOntoNode(node: NodeDef): NodeDef {
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
  node: NodeDef,
): Record<string, { model?: string; effort?: string }> | undefined {
  const resolved = resolveEditorHarness(node);
  const out: Record<string, { model?: string; effort?: string }> = {};
  // Carry over existing entries (non-resolved harnesses keep their settings).
  for (const [name, entry] of Object.entries(node.harnesses ?? {})) {
    const model = entry?.model || undefined;
    const effort = entry?.effort || undefined;
    if (model || effort) out[name] = { ...(model ? { model } : {}), ...(effort ? { effort } : {}) };
  }
  // Overlay the flat view onto the resolved harness.
  const model = node.model || undefined;
  const effort = node.effort || undefined;
  if (model || effort) {
    out[resolved] = { ...(model ? { model } : {}), ...(effort ? { effort } : {}) };
  } else {
    delete out[resolved];
  }
  return Object.keys(out).length > 0 ? out : undefined;
}
