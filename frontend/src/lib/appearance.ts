/**
 * The Apparence of an Actor (CONTEXT.md « Apparence », ADR
 * draft-l-apparence-est-une-preference-par-acteur-gardee-par-le-daemon): the
 * Theme, the Mode sombre, Nodes compactes and the deviations of the Éléments masquables.
 * Pure: no store, no request. The store is `hooks/useAppearance.ts`.
 *
 *  - **The document** the daemon keeps (`AppearanceDocument`) holds only the
 *    deviations from the defaults: a default changed later reaches whoever
 *    never touched it. {@link toDocument} and {@link fromDocument} go between
 *    it and the {@link Appearance} the interface reads.
 *  - **The local copy**: this browser keeps the last Apparence it saw under
 *    the storage keys of before (`pdo.ui.theme`, `pdo.ui.mode`,
 *    `pdo.ui.compactNodes`), so the theme script (`public/shared/theme.js`)
 *    paints the Theme and the mode before the first paint, in the interface
 *    and on a page an extension serves outside it, without a change; and the
 *    element deviations under `pdo.ui.appearance.elements`, so the other tabs
 *    follow at once.
 *  - **The migration**: an Actor with no document yet adopts the values this
 *    browser held ({@link readLocalCopy}).
 *
 * The interface is served by the daemon, so every device of an instance reads
 * the document with the same code: a Theme this interface does not know, or a
 * later `version`, only comes from a downgrade, and is read as the default.
 *
 * An element id is stable: `<surface>.<element>`, and
 * `<surface>.ext.<extension>:<contribution>` for an extension's contribution.
 * A deviation on an element no surface knows (an extension removed) is kept
 * as it is, never dropped. {@link ELEMENT_DEFAULTS} holds the core's defaults
 * per surface; an element it does not name (every extension contribution) is
 * shown by default.
 */

import type { AppearanceDocument } from "../shared/generated/AppearanceDocument";
import { COMPACT_NODES_STORAGE_KEY, loadCompactNodes, saveCompactNodes } from "./compactNodes";
import {
  DEFAULT_THEME_CHOICE,
  loadThemeChoice,
  MODE_STORAGE_KEY,
  saveMode,
  saveTheme,
  systemPrefersDark,
  THEME_STORAGE_KEY,
  THEMES,
  type Mode,
  type Theme,
} from "./theme";

/** The form of the document this interface writes. */
export const APPEARANCE_VERSION = 1;

/** The local copy of the element deviations. */
export const ELEMENTS_STORAGE_KEY = "pdo.ui.appearance.elements";

/** Every key of the local copy: a `storage` event on one of them is a change. */
export const LOCAL_COPY_KEYS: readonly string[] = [
  THEME_STORAGE_KEY,
  MODE_STORAGE_KEY,
  COMPACT_NODES_STORAGE_KEY,
  ELEMENTS_STORAGE_KEY,
];

/** Visible or not, per element id: the deviations only. */
export type ElementDeviations = Readonly<Record<string, boolean>>;

export interface Appearance {
  theme: Theme;
  mode: Mode;
  compactNodes: boolean;
  elements: ElementDeviations;
}

export const DEFAULT_APPEARANCE: Appearance = {
  ...DEFAULT_THEME_CHOICE,
  compactNodes: false,
  elements: {},
};

/** An Élément masquable as a switch of Settings › Appearance names it: its id and its label. */
export interface RowElement {
  id: string;
  label: string;
}

/** A group of a block's switches (« First line », « Stats »…). */
export interface ElementGroup {
  label: string;
  elements: readonly RowElement[];
}

/** A surface whose Éléments masquables Settings › Appearance lists. */
export type Surface = "runs.list" | "run.canvas" | "run.info" | "triggers.list" | "pipelines.list";

/**
 * The core's default of each of its Éléments masquables (the « less loaded »
 * defaults of the design): visible `true`, hidden `false`. The fixed elements of
 * a surface (a row's status and name…) are not in it: no switch hides them.
 */
export const ELEMENT_DEFAULTS: ElementDeviations = {
  "runs.list.trigger": true,
  "runs.list.child-pills": false,
  "runs.list.node-pips": false,
  "runs.list.pipeline": true,
  "runs.list.parent": false,
  "runs.list.duration": true,
  "runs.list.cost": false,
  "run.canvas.agent": false,
  "run.canvas.iteration": false,
  "run.canvas.elapsed": true,
  "run.canvas.cost": true,
  "run.canvas.isolation": false,
  "run.canvas.loop": true,
  "run.canvas.profile": true,
  "run.info.note": false,
  "run.info.status": false,
  "run.info.harness": true,
  "run.info.repo": true,
  "run.info.sandbox": true,
  "run.info.started": true,
  "run.info.trigger": true,
  "run.info.source": true,
  "run.info.duration": true,
  "run.info.sessions": false,
  "run.info.changes": true,
  "run.info.cost": true,
  "run.info.graph": false,
  "triggers.list.pipeline": true,
  "triggers.list.last": false,
  "triggers.list.next": true,
  "triggers.list.guard": true,
  "triggers.list.overlap": false,
  "pipelines.list.version": true,
  "pipelines.list.draft": true,
  "pipelines.list.modified": true,
  "pipelines.list.validation": true,
};

/**
 * The surface whose rendering of a generic UI location filters the hidden
 * contributions: each contribution there is an Élément masquable of it.
 */
export const SURFACE_OF_LOCATION: Readonly<Partial<Record<string, Surface>>> = {
  "run/item": "runs.list",
  "run/item/people": "runs.list",
  "run/info": "run.info",
};

/**
 * The element id of an extension's contribution on `surface`: `<surface>.ext.<key>`,
 * `key` being the catalogue's `<extension id>:<contribution id>`.
 */
export function contributionElementId(surface: Surface, key: string): string {
  return `${surface}.ext.${key}`;
}

const isTheme = (value: unknown): value is Theme => typeof value === "string" && (THEMES as readonly string[]).includes(value);

/**
 * The Apparence a document says; whatever it leaves out (or names a Theme this
 * interface does not know) is the default.
 */
export function fromDocument(document: AppearanceDocument): Appearance {
  return {
    theme: isTheme(document.theme) ? document.theme : DEFAULT_APPEARANCE.theme,
    mode: document.dark_mode == null ? DEFAULT_APPEARANCE.mode : document.dark_mode ? "dark" : "light",
    compactNodes: document.compact_nodes ?? DEFAULT_APPEARANCE.compactNodes,
    elements: cleanElements(document.elements),
  };
}

/** The document of an Apparence: its version and its deviations only. */
export function toDocument(appearance: Appearance): AppearanceDocument {
  const document: AppearanceDocument = { version: APPEARANCE_VERSION, elements: { ...appearance.elements } };
  if (appearance.theme !== DEFAULT_APPEARANCE.theme) document.theme = appearance.theme;
  if (appearance.mode !== DEFAULT_APPEARANCE.mode) document.dark_mode = appearance.mode === "dark";
  if (appearance.compactNodes !== DEFAULT_APPEARANCE.compactNodes) document.compact_nodes = appearance.compactNodes;
  return document;
}

/**
 * Whether the element `id` shows: its deviation, else its default in
 * `defaults`, else shown (an extension's contribution is shown by default).
 */
export function isVisible(appearance: Appearance, id: string, defaults: ElementDeviations = ELEMENT_DEFAULTS): boolean {
  return appearance.elements[id] ?? defaults[id] ?? true;
}

/**
 * Show or hide the element `id`. Back to its default, the deviation goes: only
 * what differs is kept.
 */
export function withElement(
  appearance: Appearance,
  id: string,
  visible: boolean,
  defaults: ElementDeviations = ELEMENT_DEFAULTS,
): Appearance {
  const elements = { ...appearance.elements };
  if (visible === (defaults[id] ?? true)) delete elements[id];
  else elements[id] = visible;
  return { ...appearance, elements };
}

/** Whether the element `id` differs from its default: the user touched it. */
export function isChanged(appearance: Appearance, id: string): boolean {
  return id in appearance.elements;
}

/**
 * How many choices of Settings › Appearance › Appearance differ from the
 * defaults: Nodes compactes and each element deviation. The Theme and the
 * mode are not counted: their own section sets them.
 */
export function deviationCount(appearance: Appearance): number {
  return Object.keys(appearance.elements).length + (appearance.compactNodes !== DEFAULT_APPEARANCE.compactNodes ? 1 : 0);
}

/** « Reset to defaults »: Nodes compactes and every element back to their default; the Theme and the mode stay. */
export function resetToDefaults(appearance: Appearance): Appearance {
  return { ...appearance, compactNodes: DEFAULT_APPEARANCE.compactNodes, elements: {} };
}

export function sameAppearance(a: Appearance, b: Appearance): boolean {
  if (a.theme !== b.theme || a.mode !== b.mode || a.compactNodes !== b.compactNodes) return false;
  const keys = Object.keys(a.elements);
  return keys.length === Object.keys(b.elements).length && keys.every((key) => a.elements[key] === b.elements[key]);
}

/** Only the `id → boolean` entries of a stored table. */
function cleanElements(value: unknown): ElementDeviations {
  if (!value || typeof value !== "object" || Array.isArray(value)) return {};
  const out: Record<string, boolean> = {};
  for (const [key, visible] of Object.entries(value)) if (typeof visible === "boolean") out[key] = visible;
  return out;
}

function readElements(): ElementDeviations {
  try {
    const raw = localStorage.getItem(ELEMENTS_STORAGE_KEY);
    return raw ? cleanElements(JSON.parse(raw)) : {};
  } catch {
    return {};
  }
}

/** The Apparence this browser last saw: its Theme, mode, Nodes compactes and element deviations. */
export function readLocalCopy(systemDark: boolean = systemPrefersDark()): Appearance {
  return { ...loadThemeChoice(systemDark), compactNodes: loadCompactNodes(), elements: readElements() };
}

/** Keep `appearance` as this browser's copy, under the keys the theme script reads. */
export function writeLocalCopy(appearance: Appearance): void {
  saveTheme(appearance.theme);
  saveMode(appearance.mode);
  saveCompactNodes(appearance.compactNodes);
  try {
    if (Object.keys(appearance.elements).length === 0) localStorage.removeItem(ELEMENTS_STORAGE_KEY);
    else localStorage.setItem(ELEMENTS_STORAGE_KEY, JSON.stringify(appearance.elements));
  } catch {
    // quota / disabled / private mode → in-memory only for this session
  }
}
