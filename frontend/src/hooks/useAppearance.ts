/**
 * The live Apparence of the Actor at the keyboard (CONTEXT.md « Apparence »):
 * one module store, read by every surface (`useAppearance`, and `useTheme`,
 * `useCompactNodes` over it), so the Settings rows, the command field and the
 * canvases never disagree.
 *
 *  - **Boot** ({@link initAppearance}, before the first render): the local
 *    copy of this browser, painted on `<html>`. The theme script stamped the
 *    same Theme, mode and status palette already: no flash.
 *  - **The daemon** ({@link syncAppearance}, once the interface is admitted,
 *    and again on every resync): its document wins and becomes the local copy;
 *    an Actor with no document yet adopts this browser's values, written back
 *    at once (the migration). A change made before the answer wins over it.
 *  - **A change** ({@link updateAppearance}): painted, kept as the local copy,
 *    then written to the daemon, the latest only when changes queue up.
 *  - **Another tab** of this browser writes the local copy: its `storage`
 *    event repaints this one at once. Another device reads the daemon at its
 *    next load or resync.
 */
import { useSyncExternalStore } from "react";
import {
  DEFAULT_APPEARANCE,
  LOCAL_COPY_KEYS,
  fromDocument,
  readLocalCopy,
  sameAppearance,
  toDocument,
  writeLocalCopy,
  type Appearance,
} from "../lib/appearance";
import { applyStatusPalette } from "../lib/statusPalette";
import { applyThemeChoice } from "../lib/theme";
import { fetchAppearance, saveAppearance } from "../settings/api/appearance";
import { toast } from "../shared/ui/toast";

let snapshot: Appearance = DEFAULT_APPEARANCE;
let loaded = false;
/** Bumped on every change, here or in another tab: a daemon answer older than one is dropped. */
let changes = 0;
/** A change of this tab the daemon has not confirmed yet (on its way, or refused). */
let unsaved = false;
const listeners = new Set<() => void>();
let unbindStorage: (() => void) | null = null;

function publish(next: Appearance): void {
  if (sameAppearance(next, snapshot)) return;
  snapshot = next;
  paint(snapshot);
  for (const notify of listeners) notify();
}

/** The Theme, the mode and the status palette on `<html>`. */
function paint(appearance: Appearance): void {
  applyThemeChoice(appearance);
  applyStatusPalette(appearance.colourBlindStatus);
}

function current(): Appearance {
  if (!loaded) {
    loaded = true;
    snapshot = readLocalCopy();
  }
  return snapshot;
}

/**
 * Read this browser's copy, paint it, and follow the other tabs. Called once
 * at boot, before the first render.
 */
export function initAppearance(): Appearance {
  unbindStorage?.();
  unbindStorage = null;
  loaded = true;
  snapshot = readLocalCopy();
  paint(snapshot);
  for (const notify of listeners) notify();
  followOtherTabs();
  return snapshot;
}

/** Repaint on a change another tab made to the local copy. */
function followOtherTabs(): void {
  if (unbindStorage || typeof window === "undefined") return;
  const onStorage = (event: StorageEvent) => {
    if (event.key === null || LOCAL_COPY_KEYS.includes(event.key)) {
      // Newer than a daemon answer on its way to this tab.
      changes += 1;
      publish(readLocalCopy());
    }
  };
  window.addEventListener("storage", onStorage);
  unbindStorage = () => window.removeEventListener("storage", onStorage);
}

let writing: Promise<void> | null = null;
let queued: Appearance | null = null;
let warned = false;

/** Write `appearance` to the daemon; while a write is on its way, only the latest waits. */
function persist(appearance: Appearance): Promise<void> {
  queued = appearance;
  writing ??= (async () => {
    while (queued) {
      const next = queued;
      queued = null;
      try {
        await saveAppearance(toDocument(next));
        warned = false;
        if (!queued) unsaved = false;
      } catch {
        // The local copy holds the change; the next change, or the next sync, writes it again.
        if (!warned) toast("Your appearance could not be saved", "error");
        warned = true;
      }
    }
    writing = null;
  })();
  return writing;
}

/**
 * Read the Actor's Apparence from the daemon. Without one, this browser's
 * values become the Actor's. A daemon that does not answer leaves the local
 * copy on screen. A change of this tab the daemon has not confirmed yet is
 * newer than what it keeps: written again, never overwritten.
 */
export async function syncAppearance(): Promise<void> {
  if (unsaved) {
    await persist(current());
    return;
  }
  const asked = changes;
  let stored;
  try {
    stored = (await fetchAppearance()).appearance;
  } catch {
    return;
  }
  // Changed here or in another tab while the daemon answered: that change is the newer.
  if (asked !== changes || unsaved) return;
  if (stored == null) {
    await persist(current());
    return;
  }
  const next = fromDocument(stored);
  writeLocalCopy(next);
  publish(next);
}

/** Change the Apparence: painted, kept here, written to the daemon. */
export function updateAppearance(change: (appearance: Appearance) => Appearance): void {
  const next = change(current());
  if (sameAppearance(next, current())) return;
  changes += 1;
  unsaved = true;
  writeLocalCopy(next);
  publish(next);
  void persist(next);
}

/** Forget the store, so the next read goes back to the local copy (tests). */
export function resetAppearance(): void {
  unbindStorage?.();
  unbindStorage = null;
  loaded = false;
  snapshot = DEFAULT_APPEARANCE;
  changes = 0;
  unsaved = false;
  queued = null;
  writing = null;
  warned = false;
}

function subscribe(notify: () => void): () => void {
  listeners.add(notify);
  followOtherTabs();
  return () => listeners.delete(notify);
}

/** The Apparence on screen. */
export function useAppearance(): Appearance {
  return useSyncExternalStore(subscribe, current, current);
}
