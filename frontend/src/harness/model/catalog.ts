// The harness catalog, client side (#550, #586, #616, ADR-0046, ADR-0053).
//
// The daemon owns the truth (descriptor + resolver + the offer probed from each
// binary); this module is the thin client mirror the pickers need: which
// harnesses exist, whether each is installed, and what model and effort each
// offers.

import type { HarnessDescriptorsView } from "./harness";
import type { AgentCombination } from "../../settings/model/settings";

/** The floor of the precedence chain — a node with no pin runs on `claude`. */
export const HARNESS_FLOOR = "claude";

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
  /** #705: context window per offered id, served verbatim (`pi --list-models`).
   *  Empty when the source names none. */
  modelContexts: Record<string, string>;
  /** Offered effort levels, served. Empty ⇒ no effort axis. */
  efforts: string[];
  /** #798: the effort levels an exact model id supports, keyed by that id
   *  verbatim. A key PRESENT for a model is authoritative for it (`[]` included);
   *  a key MISSING falls back to {@link HarnessOption.efforts}. Optional so a
   *  HarnessOption literal predating #798 still typechecks; absent ⇒ the global
   *  `efforts` apply to every model. */
  modelEfforts?: Record<string, string[]>;
  /** The served effort-axis fact — whether to grey the effort picker. */
  hasEffort: boolean;
  /** The probed binary version the offer was read at, for the provenance line. */
  version: string | null;
  /** When the daemon last probed this harness's catalogue (RFC 3339); `null`
   *  when never. Optional so a literal predating it still typechecks. */
  probedAt?: string | null;
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
    { name: "claude", installed: true, models: [], modelContexts: {}, efforts: [], modelEfforts: {}, hasEffort: true, version: null },
    { name: "opencode", installed: true, models: [], modelContexts: {}, efforts: [], modelEfforts: {}, hasEffort: false, version: null },
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
      modelContexts: h.model_contexts ?? {},
      efforts: h.efforts ?? [],
      // #798: the per-model effort support, served. Defaulted for a daemon
      // predating #798: absent ⇒ every model falls back to the global efforts.
      modelEfforts: h.model_efforts ?? {},
      hasEffort: h.has_effort ?? true,
      version: h.version ?? null,
      probedAt: h.probed_at ?? null,
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

/** The selected model's offer. Missing capability data falls back to the
 *  harness list without claiming that other stored values are unsupported. */
export interface EffortOffer {
  /** The effort stops the picker renders as regular supported options. */
  levels: string[];
  /** True only for a model-specific answer, including an empty list.
   *  Unsupported stored values remain visible with a warning. */
  authoritative: boolean;
}

/** Compute the offer for a harness option and the model the surface selected or
 *  resolved (the flat `model` the pickers edit — see `foldHarnessOntoNode`). */
export function effortOffer(
  option: HarnessOption | undefined,
  model: string | null | undefined,
): EffortOffer {
  if (!option) return { levels: [], authoritative: false };
  if (
    model &&
    option.modelEfforts &&
    Object.prototype.hasOwnProperty.call(option.modelEfforts, model)
  ) {
    // Present key — `[]` included — is authoritative for this model.
    return { levels: option.modelEfforts[model], authoritative: true };
  }
  // A harness-wide list does not establish support for an unknown model.
  return { levels: option.efforts, authoritative: false };
}

/** The harness an editor node resolves to, with only the tiers a client knows:
 *  the node's pin, else the `claude` floor. (The instance default is a coarser
 *  tier the editor does not fetch per-node; the floor is the safe editor default,
 *  and the daemon re-resolves authoritatively at spawn.) An empty pin is "unset". */
export function resolveEditorHarness(node: { pin_harness?: string | null }): string {
  const pin = node.pin_harness;
  return pin && pin !== "" ? pin : HARNESS_FLOOR;
}

/** `4 models`, `1 model`, `no model listed`: the size of a served catalogue. */
export function modelCount(n: number): string {
  if (n === 0) return "no model listed";
  return `${n} model${n === 1 ? "" : "s"}`;
}

/** `probed just now`, `probed 12 min ago`, `probed 3 days ago`: when the
 *  catalogue was last probed. `not probed` for a harness never probed. */
export function probedAgo(probedAt: string | null | undefined, now = new Date()): string {
  const t = probedAt ? Date.parse(probedAt) : Number.NaN;
  if (Number.isNaN(t)) return "not probed";
  const s = Math.max(0, Math.round((now.getTime() - t) / 1000));
  if (s < 60) return "probed just now";
  if (s < 3600) return `probed ${Math.round(s / 60)} min ago`;
  if (s < 86400) return `probed ${Math.round(s / 3600)} h ago`;
  const days = Math.round(s / 86400);
  return `probed ${days} day${days === 1 ? "" : "s"} ago`;
}

/**
 * A combination a node can start Custom from: the given harness, the model the
 * harness lists first and its medium effort when it offers them (`null`, the
 * harness's own default, otherwise).
 */
export function customDefaults(catalog: HarnessCatalog, from: AgentCombination): AgentCombination {
  const option = findHarnessOption(catalog, from.harness);
  const model = from.model ?? option?.models[0] ?? null;
  const levels = effortOffer(option, model).levels;
  const effort = from.effort ?? (levels.includes("medium") ? "medium" : null);
  return { harness: from.harness, model, effort };
}
