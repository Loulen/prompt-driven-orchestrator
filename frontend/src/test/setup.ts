import "@testing-library/jest-dom/vitest";
import { afterEach } from "vitest";
import { useSelectionStore } from "../stores/selectionStore";

// Node ≥ 22 ships its own `localStorage` / `sessionStorage` on `globalThis`
// (the experimental Web Storage, backed by `--localstorage-file`). Without that
// flag the object exists but has none of the Storage methods, and vitest's jsdom
// environment does not overwrite a global that is already defined — so every
// test calling `localStorage.clear()` / `getItem()` blew up with "is not a
// function" under Node 25. Replace any such stub with an in-memory Storage so
// the suite behaves the same on every Node version.
class MemoryStorage implements Storage {
  private map = new Map<string, string>();
  get length() {
    return this.map.size;
  }
  clear() {
    this.map.clear();
  }
  getItem(key: string) {
    return this.map.has(key) ? (this.map.get(key) as string) : null;
  }
  key(index: number) {
    return Array.from(this.map.keys())[index] ?? null;
  }
  removeItem(key: string) {
    this.map.delete(key);
  }
  setItem(key: string, value: string) {
    this.map.set(key, String(value));
  }
}

for (const name of ["localStorage", "sessionStorage"] as const) {
  const current = (globalThis as Record<string, unknown>)[name] as Partial<Storage> | undefined;
  if (!current || typeof current.clear !== "function" || typeof current.getItem !== "function") {
    Object.defineProperty(globalThis, name, {
      value: new MemoryStorage(),
      configurable: true,
      writable: true,
    });
  }
}

// #577 — the multi-select store is a module singleton shared across every test;
// reset it after each so a selection made in one test can't leak a floating bar
// or a tab badge into the next.
afterEach(() => {
  useSelectionStore.getState().clearAll();
});
