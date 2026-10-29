import "@testing-library/jest-dom/vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { configure } from "@testing-library/react";
import { afterEach } from "vitest";
import { useSelectionStore } from "../stores/selectionStore";

// `waitFor` / `findBy*` give up after 1 s by default. On a loaded machine (load
// ~37, parallel workers) a first render with lazy imports takes longer:
// ReviewPage's "loads fork → tip by default" failed `make test` at 2.5 s. 5 s,
// under the 20 s `testTimeout`; a wait that succeeds still returns at once.
configure({ asyncUtilTimeout: 5_000 });

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

// The tokens read resolved by the surfaces painted out of CSS (`lib/cssColor`):
// the terminal's palette and the harnesses' series colours. jsdom loads no
// stylesheet, so their first block is declared on <html>, as the browser does.
for (const sheet of ["../session/terminal.css", "../harness/harness.css"]) {
  const css = readFileSync(join(import.meta.dirname, sheet), "utf8");
  const block = css.slice(css.indexOf("{") + 1, css.indexOf("}"));
  for (const [, name, value] of block.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) {
    document.documentElement.style.setProperty(name, value.trim());
  }
}
