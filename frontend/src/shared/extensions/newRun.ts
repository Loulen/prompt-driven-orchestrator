/**
 * The seam « open New Run prefilled » (API 0.22): an extension's UI opens the
 * core's New Run form with a partial prefill (`NewRunPrefill`), through
 * `openNewRun` of `@pdo/extension-ui` or `CommandHost.openNewRun`. The app
 * shell registers what opens its form (`registerNewRunOpener`); without one
 * (the Review page), nothing opens.
 */

import type { NewRunPrefill } from "./contract";

/** Opens the form, or answers `false` when it cannot (already open). */
type Opener = (prefill: NewRunPrefill | undefined) => boolean;

let opener: Opener | null = null;

/** The app shell's New Run, while it is mounted. Returns its unregistration. */
export function registerNewRunOpener(open: Opener): () => void {
  opener = open;
  return () => {
    if (opener === open) opener = null;
  };
}

function text(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() !== "" ? value : undefined;
}

/**
 * What of `raw` the form takes: the fields of the right type, a non-empty
 * text each; `undefined` when none is left (a plain open).
 */
export function cleanPrefill(raw: unknown): NewRunPrefill | undefined {
  if (raw === null || typeof raw !== "object") return undefined;
  const r = raw as Record<string, unknown>;
  const repositories = Array.isArray(r.repositories)
    ? r.repositories.filter((repo): repo is string => text(repo) !== undefined).map((repo) => repo.trim())
    : [];
  const extension = text(r.extension);
  const prefill: NewRunPrefill = {
    ...(text(r.project) ? { project: text(r.project) } : {}),
    ...(repositories.length > 0 ? { repositories } : {}),
    ...(text(r.name) ? { name: text(r.name) } : {}),
    ...(text(r.input) ? { input: text(r.input) } : {}),
    ...(extension && r.data !== undefined ? { extension, data: r.data } : {}),
  };
  return Object.keys(prefill).length > 0 ? prefill : undefined;
}

/**
 * Open New Run with `prefill` (every field optional); without one, as the
 * core's New Run button does. `false` where no New Run can open, and while
 * one is already open (its draft is left as it is).
 */
export function openNewRun(prefill?: NewRunPrefill): boolean {
  if (!opener) return false;
  return opener(cleanPrefill(prefill));
}
