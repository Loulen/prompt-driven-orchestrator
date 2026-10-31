/**
 * The live « Compact nodes » preference: one module store, read by
 * `useCompactNodes()`, so the Settings row, the command field and every open
 * Run canvas never disagree. Another tab of the same daemon that changes it
 * writes the store; its `storage` event redraws this one too.
 */
import { useSyncExternalStore } from "react";
import { COMPACT_NODES_STORAGE_KEY, loadCompactNodes, saveCompactNodes } from "../lib/compactNodes";

let snapshot: boolean | null = null;
const listeners = new Set<() => void>();
let unbindStorage: (() => void) | null = null;

const current = (): boolean => (snapshot ??= loadCompactNodes());

function publish(next: boolean): void {
  if (next === current()) return;
  snapshot = next;
  for (const notify of listeners) notify();
}

function subscribe(notify: () => void): () => void {
  listeners.add(notify);
  if (!unbindStorage && typeof window !== "undefined") {
    const onStorage = (event: StorageEvent) => {
      if (event.key === COMPACT_NODES_STORAGE_KEY || event.key === null) publish(loadCompactNodes());
    };
    window.addEventListener("storage", onStorage);
    unbindStorage = () => window.removeEventListener("storage", onStorage);
  }
  return () => {
    listeners.delete(notify);
    if (listeners.size === 0) {
      unbindStorage?.();
      unbindStorage = null;
    }
  };
}

export function setCompactNodes(on: boolean): void {
  saveCompactNodes(on);
  publish(on);
}

export function toggleCompactNodes(): void {
  setCompactNodes(!current());
}

/** Drops the cached value, so the next read goes back to the store (tests). */
export function resetCompactNodes(): void {
  snapshot = null;
}

export function useCompactNodes(): { compactNodes: boolean; setCompactNodes: (on: boolean) => void; toggleCompactNodes: () => void } {
  const compactNodes = useSyncExternalStore(subscribe, current, current);
  return { compactNodes, setCompactNodes, toggleCompactNodes };
}
