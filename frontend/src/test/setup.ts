import "@testing-library/jest-dom/vitest";
import { afterEach } from "vitest";
import { useSelectionStore } from "../stores/selectionStore";

// Node ≥ 22 ships an experimental `globalThis.localStorage` (a Web Storage
// stub backed by `--localstorage-file`) that shadows jsdom's real Storage in
// the vitest global — it has no `clear`, `key`, `length` and cannot be spied
// on, which broke ~180 tests on Node 25. Replace it with a plain in-memory
// Storage on both `globalThis` and `window` before any test module loads.
class MemoryStorage implements Storage {
  private map = new Map<string, string>();
  get length(): number { return this.map.size; }
  clear(): void { this.map.clear(); }
  getItem(key: string): string | null { return this.map.has(key) ? this.map.get(key)! : null; }
  key(index: number): string | null { return Array.from(this.map.keys())[index] ?? null; }
  removeItem(key: string): void { this.map.delete(key); }
  setItem(key: string, value: string): void { this.map.set(key, String(value)); }
}

const needsStorageShim =
  typeof globalThis.localStorage === "undefined" ||
  typeof (globalThis.localStorage as Partial<Storage>).clear !== "function";
if (needsStorageShim) {
  for (const name of ["localStorage", "sessionStorage"] as const) {
    const store = new MemoryStorage();
    Object.defineProperty(globalThis, name, { value: store, configurable: true, writable: true });
    if (typeof window !== "undefined" && window !== (globalThis as unknown as Window)) {
      Object.defineProperty(window, name, { value: store, configurable: true, writable: true });
    }
  }
}

// #577 — the multi-select store is a module singleton shared across every test;
// reset it after each so a selection made in one test can't leak a floating bar
// or a tab badge into the next.
afterEach(() => {
  useSelectionStore.getState().clearAll();
});
