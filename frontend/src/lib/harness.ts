// The agentic-harness axis, client side (#550, ADR-0046).
//
// The daemon owns the truth (descriptor + resolver); this module is the thin
// client mirror the editor needs: which harnesses exist, which one an editor node
// resolves to, whether a harness exposes an effort axis (to grey the picker), and
// the fold between the node's per-harness `harnesses` map and the single
// `model`/`effort` view the existing pickers edit.

import type { HarnessDescriptorsView, NodeDef } from "../types";

/** The floor of the precedence chain — a node with no pin runs on `claude`. */
export const HARNESS_FLOOR = "claude";

const PINNED_HARNESS_COLORS: Record<string, string> = {
  copilot: "#58a6ff",
  claude: "#f0883e",
};

/** Stable series colour shared by every harness visualization. */
export function harnessColor(name: string): string {
  const normalized = name.trim().toLowerCase();
  const pinned = PINNED_HARNESS_COLORS[normalized];
  if (pinned) return pinned;

  let hash = 2166136261;
  for (const char of normalized) {
    hash ^= char.codePointAt(0) ?? 0;
    hash = Math.imul(hash, 16777619);
  }
  return `hsl(${(hash >>> 0) % 360} 64% 58%)`;
}

/** One harness as the picker offers it (#586): its name and whether its binary is
 *  installed (an uninstalled harness renders greyed and non-selectable). */
export interface HarnessOption {
  name: string;
  installed: boolean;
}

/** The picker's two sections (#586): the embedded floor, and the disk-declared
 *  tier. Names only — no capability pills (ADR of #586: directions B/C/D dropped). */
export interface HarnessCatalog {
  builtin: HarnessOption[];
  descriptors: HarnessOption[];
}

/** The floor the picker shows before `GET /settings` answers, or against a daemon
 *  that predates #586: the embedded harnesses, assumed installed. This is NOT the
 *  picker's source of truth (that is `view.harnesses`) — it is the transient
 *  fallback so the control is never empty while settings load. */
const FLOOR_CATALOG: HarnessCatalog = {
  builtin: [
    { name: "claude", installed: true },
    { name: "opencode", installed: true },
  ],
  descriptors: [],
};

/** Split `GET /settings → harness_descriptors` into the picker's two sections
 *  (#586). `source` decides the section; refused descriptors never resolve, so
 *  they are already absent from `harnesses`. Falls back to the embedded floor when
 *  the view (or its `harnesses` field) is missing — a still-loading fetch or a
 *  daemon predating #586. */
export function harnessCatalog(
  view: HarnessDescriptorsView | null | undefined,
): HarnessCatalog {
  if (!view?.harnesses) return FLOOR_CATALOG;
  const catalog: HarnessCatalog = { builtin: [], descriptors: [] };
  for (const h of view.harnesses) {
    const section = h.source === "descriptor" ? catalog.descriptors : catalog.builtin;
    section.push({ name: h.name, installed: h.installed });
  }
  return catalog;
}

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
