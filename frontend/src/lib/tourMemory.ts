/**
 * What this browser remembers about tours (#823, spec #821).
 *
 * Two keys, in the `pdo.*` localStorage namespace the rest of the client uses
 * (`lib/uiPrefs.ts`, `lib/dismissedBanners.ts`): whether the welcome modal has
 * already been answered, and which tours have been *finished*. Deliberately
 * per-browser and not `instance_config` — the same reasoning as ADR-0015 in
 * `uiPrefs.ts`: two browsers pointed at one daemon would fight over one row, and
 * "have I seen the welcome?" is a fact about a reader, not about an instance.
 *
 * Both read and write are guarded: private mode or a full store degrades to an
 * in-memory default for the session rather than throwing in the middle of a tour.
 */

/** Set once the welcome modal has been answered — by ANY of its choices, "Later"
 *  included. That is the whole point of story 3: « plus tard » is never re-asked. */
const OFFERED_KEY = "pdo.tour.offered";

/**
 * **One key per finished tour** (spec #821), holding the day it was finished —
 * the same discipline as `dismissedBanners.ts`'s per-pipeline key. A single map
 * would be one blob to corrupt: one unreadable entry would erase every checkmark,
 * where here it costs exactly the tour it belongs to.
 *
 * Only ever written when a tour reaches its end card: quitting mid-tour marks
 * nothing (CONTEXT.md § « Étape de tour »).
 */
const DONE_PREFIX = "pdo.tour.done.";

export function loadTourOffered(): boolean {
  try {
    return localStorage.getItem(OFFERED_KEY) === "1";
  } catch {
    return false;
  }
}

export function markTourOffered(): void {
  try {
    localStorage.setItem(OFFERED_KEY, "1");
  } catch {
    // quota / disabled / private mode → in-memory only for this session
  }
}

/** Every `pdo.tour.done.*` key currently in the store. */
function doneKeys(): string[] {
  const keys: string[] = [];
  try {
    for (let i = 0; i < localStorage.length; i++) {
      const key = localStorage.key(i);
      if (key?.startsWith(DONE_PREFIX)) keys.push(key);
    }
  } catch {
    // unreadable store → nothing finished
  }
  return keys;
}

/** Finished tours, by id, with the day they were finished. An unreadable entry
 *  reads as "not finished": a broken checkmark must never hide a Start button. */
export function loadToursDone(): Record<string, string> {
  const out: Record<string, string> = {};
  for (const key of doneKeys()) {
    try {
      const at = localStorage.getItem(key);
      if (at) out[key.slice(DONE_PREFIX.length)] = at;
    } catch {
      // skip this tour only
    }
  }
  return out;
}

export function markTourDone(tourId: string, at: Date = new Date()): Record<string, string> {
  try {
    localStorage.setItem(DONE_PREFIX + tourId, at.toISOString());
  } catch {
    // quota / disabled / private mode → in-memory only for this session
  }
  return loadToursDone();
}

/** Settings › Tutorials › « Reset tutorial memory »: forget the checkmarks AND the
 *  welcome prompt, so the next load proposes the modal again on a Run-less instance. */
export function resetTourMemory(): void {
  try {
    localStorage.removeItem(OFFERED_KEY);
    for (const key of doneKeys()) localStorage.removeItem(key);
  } catch {
    // nothing to forget if the store is unreadable
  }
}

export interface WelcomeInput {
  /** The browser already carries the "tutorial offered" key. */
  offered: boolean;
  /** The run list has actually been fetched — `runCount` is meaningless before. */
  runsLoaded: boolean;
  runCount: number;
}

/**
 * The welcome-modal rule (spec #821, stories 1–4): propose a tour only on a
 * browser that was never asked **and** an instance with no Run at all. A seasoned
 * user opening a fresh browser on a working instance is not a beginner.
 *
 * The `runsLoaded` guard is not a detail: without it the modal would flash on every
 * load, in the gap before `GET /runs` answers, on every instance.
 */
export function shouldOfferWelcome({ offered, runsLoaded, runCount }: WelcomeInput): boolean {
  return !offered && runsLoaded && runCount === 0;
}
