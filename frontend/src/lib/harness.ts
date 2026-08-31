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

/** One harness as the picker offers it (#586, #616): its name, whether its binary
 *  is installed (an uninstalled harness renders greyed and non-selectable), and the
 *  offer the daemon deduced from that binary — the model & effort catalogues, the
 *  served effort-axis fact, and the probed version. The catalogues drive the model
 *  and effort pickers; `hasEffort` greys the effort picker (no more client-side
 *  map). Empty catalogues mean the binary enumerates none → free text. */
export interface HarnessOption {
  name: string;
  installed: boolean;
  /** Offered model ids, served (ADR-0053). Empty ⇒ free-text model. */
  models: string[];
  /** Offered effort levels, served. Empty ⇒ no effort axis. */
  efforts: string[];
  /** The served effort-axis fact — whether to grey the effort picker. */
  hasEffort: boolean;
  /** The probed binary version the offer was read at, for the provenance line. */
  version: string | null;
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
    // The transient pre-fetch floor carries no catalogue — models/efforts arrive
    // with `GET /settings`. `hasEffort` seeds the two embedded facts so the picker
    // is not mis-greyed for the split-second before the fetch resolves; the served
    // value replaces it. Not a source of truth (the daemon re-resolves at spawn).
    { name: "claude", installed: true, models: [], efforts: [], hasEffort: true, version: null },
    { name: "opencode", installed: true, models: [], efforts: [], hasEffort: false, version: null },
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
    section.push({
      name: h.name,
      installed: h.installed,
      // #616/ADR-0053: the offer the daemon deduced from the binary. Defaulted for a
      // daemon predating #616 (no fields served): empty catalogues → free text, and
      // `has_effort` absent → don't grey (the old conservative default) — never a
      // client-side per-name map.
      models: h.models ?? [],
      efforts: h.efforts ?? [],
      hasEffort: h.has_effort ?? true,
      version: h.version ?? null,
    });
  }
  return catalog;
}

/** Find a harness's option by name across both sections, or `undefined` for a name
 *  the catalogue does not carry (an unknown pin — the daemon re-resolves at spawn).
 *  This is how a surface reads the served offer (models, efforts, effort axis) for
 *  the harness a node resolves to. */
export function findHarnessOption(
  catalog: HarnessCatalog,
  name: string,
): HarnessOption | undefined {
  return (
    catalog.builtin.find((o) => o.name === name) ??
    catalog.descriptors.find((o) => o.name === name)
  );
}

/** Whether the harness a node resolves to greys its effort picker. Reads the SERVED
 *  effort-axis fact off the catalogue (ADR-0053), no hard-coded map. An unknown
 *  harness (absent from the catalogue) defaults to "has effort" — the conservative
 *  choice: don't grey what we can't see; the daemon ignores an effort a harness
 *  can't honour anyway. */
export function harnessHasEffort(catalog: HarnessCatalog, name: string): boolean {
  return findHarnessOption(catalog, name)?.hasEffort ?? true;
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
