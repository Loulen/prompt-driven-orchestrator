/**
 * Stats' per-browser preferences (#810), in the `pdo.stats.*` localStorage
 * namespace.
 *
 * Same nature and same discipline as the theme and `uiPrefs` (#342): a
 * presentation preference has no daemon-wide scope — two browsers pointed at
 * one daemon would fight over a stored row — so it never reaches
 * `instance_config` (ADR-0015). Read AND write are guarded: private mode, a
 * disabled store or a full quota degrades to the default for the session
 * instead of throwing.
 *
 * The defaults here are the **frontend's**, not the wire's. `/stats/*` keeps
 * `completed_only=false` as its wire default (an older client must keep the
 * cohort it always had); a fresh browser opens Stats on the reading the design
 * settled on — completed runs only, declared waits subtracted, every node kind
 * shown.
 */

/** The node **genre** a Performance row can carry (CONTEXT.md § Genre de nœud).
 *  A node that is both interactive and orchestrator matches either chip;
 *  « standard » is the absence of both, never a third stored flag. */
export type NodeKind = "interactive" | "orchestrator" | "standard";

export const ALL_NODE_KINDS: readonly NodeKind[] = [
  "interactive",
  "orchestrator",
  "standard",
];

/** Defaults a fresh browser opens Stats with. */
export const DEFAULT_COMPLETED_ONLY = true;
export const DEFAULT_EXCLUDE_USER_WAIT = true;

const COMPLETED_ONLY_KEY = "pdo.stats.completed_only";
const EXCLUDE_USER_WAIT_KEY = "pdo.stats.exclude_user_wait";
const NODE_KINDS_KEY = "pdo.stats.node_kinds";

function loadBoolean(key: string, fallback: boolean): boolean {
  try {
    const raw = localStorage.getItem(key);
    if (raw == null) return fallback;
    const value: unknown = JSON.parse(raw);
    return typeof value === "boolean" ? value : fallback;
  } catch {
    return fallback;
  }
}

function save(key: string, value: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // quota / disabled / private mode → in-memory only for this session
  }
}

export function loadCompletedOnly(): boolean {
  return loadBoolean(COMPLETED_ONLY_KEY, DEFAULT_COMPLETED_ONLY);
}

export function saveCompletedOnly(value: boolean): void {
  save(COMPLETED_ONLY_KEY, value);
}

export function loadExcludeUserWait(): boolean {
  return loadBoolean(EXCLUDE_USER_WAIT_KEY, DEFAULT_EXCLUDE_USER_WAIT);
}

export function saveExcludeUserWait(value: boolean): void {
  save(EXCLUDE_USER_WAIT_KEY, value);
}

/** The checked kinds, in the canonical order. An absent, unparseable or
 *  foreign-valued entry reads as « all kinds » — a stored preference is never
 *  allowed to hide rows the user cannot see a reason for. An empty stored array
 *  IS meaningful (the user unchecked everything) and is kept. */
export function loadNodeKinds(): NodeKind[] {
  try {
    const raw = localStorage.getItem(NODE_KINDS_KEY);
    if (raw == null) return [...ALL_NODE_KINDS];
    const value: unknown = JSON.parse(raw);
    if (!Array.isArray(value)) return [...ALL_NODE_KINDS];
    const kinds = ALL_NODE_KINDS.filter((kind) => value.includes(kind));
    // A non-empty entry naming nothing we know is corrupt, not a choice: fall
    // back to every kind rather than render an empty Performance tab.
    return kinds.length === 0 && value.length > 0 ? [...ALL_NODE_KINDS] : kinds;
  } catch {
    return [...ALL_NODE_KINDS];
  }
}

export function saveNodeKinds(kinds: NodeKind[]): void {
  save(
    NODE_KINDS_KEY,
    ALL_NODE_KINDS.filter((kind) => kinds.includes(kind)),
  );
}

/** Do the Performance filters deviate from the defaults? Drives the amber cue
 *  on the Performance rail entry and the « reset filters » link: the settings
 *  survive a reload, so without a cue nobody would know why the numbers moved. */
export function performanceFiltersDeviate(
  excludeUserWait: boolean,
  kinds: NodeKind[],
): boolean {
  return (
    excludeUserWait !== DEFAULT_EXCLUDE_USER_WAIT ||
    kinds.length !== ALL_NODE_KINDS.length
  );
}
