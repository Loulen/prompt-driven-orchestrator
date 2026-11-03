import { beforeEach, describe, expect, it } from "vitest";
import {
  DEFAULT_APPEARANCE,
  deviationCount,
  ELEMENTS_STORAGE_KEY,
  fromDocument,
  isVisible,
  readLocalCopy,
  resetToDefaults,
  toDocument,
  withElement,
  writeLocalCopy,
  type Appearance,
} from "./appearance";

beforeEach(() => localStorage.clear());

describe("the Apparence module", () => {
  it("defaults to Acogni, dark, detailed cards and no deviation", () => {
    expect(DEFAULT_APPEARANCE).toEqual({ theme: "acogni", mode: "dark", compactNodes: false, elements: {} });
    expect(fromDocument({ version: 1, elements: {} })).toEqual(DEFAULT_APPEARANCE);
  });

  it("writes only the deviations, and reads them back", () => {
    expect(toDocument(DEFAULT_APPEARANCE)).toEqual({ version: 1, elements: {} });
    const mine: Appearance = { theme: "classic", mode: "light", compactNodes: true, elements: { "runs.list.cost": true } };
    const document = toDocument(mine);
    expect(document).toEqual({
      version: 1,
      theme: "classic",
      dark_mode: false,
      compact_nodes: true,
      elements: { "runs.list.cost": true },
    });
    expect(fromDocument(document)).toEqual(mine);
  });

  it("reads a Theme it does not know as the default one, and drops a non-boolean deviation", () => {
    const read = fromDocument({ version: 2, theme: "tomorrow", dark_mode: null, elements: { a: true, b: "no" as unknown as boolean } });
    expect(read.theme).toBe("acogni");
    expect(read.mode).toBe("dark");
    expect(read.elements).toEqual({ a: true });
  });

  it("reads an element from its deviation, else its default, else shown", () => {
    const defaults = { "runs.list.cost": false, "runs.list.pipeline": true };
    const a = { ...DEFAULT_APPEARANCE, elements: { "runs.list.cost": true } };
    expect(isVisible(a, "runs.list.cost", defaults)).toBe(true);
    expect(isVisible(a, "runs.list.pipeline", defaults)).toBe(true);
    expect(isVisible(DEFAULT_APPEARANCE, "runs.list.cost", defaults)).toBe(false);
    // An extension's contribution: shown by default.
    expect(isVisible(DEFAULT_APPEARANCE, "runs.list.ext.test:mark", defaults)).toBe(true);
  });

  it("keeps a deviation only while it differs from the default", () => {
    const defaults = { "runs.list.cost": false };
    const shown = withElement(DEFAULT_APPEARANCE, "runs.list.cost", true, defaults);
    expect(shown.elements).toEqual({ "runs.list.cost": true });
    expect(withElement(shown, "runs.list.cost", false, defaults).elements).toEqual({});
  });

  it("keeps a deviation on an element nobody knows any more", () => {
    const stored = { version: 1, elements: { "runs.list.ext.gone:mark": false } };
    const read = fromDocument(stored);
    expect(read.elements).toEqual({ "runs.list.ext.gone:mark": false });
    const changed = withElement(read, "runs.list.cost", true, { "runs.list.cost": false });
    expect(toDocument(changed).elements).toEqual({ "runs.list.ext.gone:mark": false, "runs.list.cost": true });
  });

  it("counts and resets Compact nodes and the elements, never the Theme nor the mode", () => {
    const mine: Appearance = { theme: "classic", mode: "light", compactNodes: true, elements: { "runs.list.cost": true } };
    expect(deviationCount(mine)).toBe(2);
    expect(deviationCount({ ...DEFAULT_APPEARANCE, theme: "classic" })).toBe(0);
    expect(resetToDefaults(mine)).toEqual({ theme: "classic", mode: "light", compactNodes: false, elements: {} });
  });

  it("migrates the values this browser held", () => {
    localStorage.setItem("pdo.ui.theme", "classic");
    localStorage.setItem("pdo.ui.mode", "light");
    localStorage.setItem("pdo.ui.compactNodes", "true");
    expect(readLocalCopy()).toEqual({ theme: "classic", mode: "light", compactNodes: true, elements: {} });
    // A value of before 2.68 is migrated on the way, as the theme script does.
    localStorage.clear();
    localStorage.setItem("pdo.ui.theme", "light");
    expect(readLocalCopy(true)).toEqual({ ...DEFAULT_APPEARANCE, mode: "light" });
  });

  it("keeps its local copy under the keys the theme script reads", () => {
    writeLocalCopy({ theme: "classic", mode: "light", compactNodes: true, elements: { "runs.list.cost": true } });
    expect(localStorage.getItem("pdo.ui.theme")).toBe("classic");
    expect(localStorage.getItem("pdo.ui.mode")).toBe("light");
    expect(localStorage.getItem("pdo.ui.compactNodes")).toBe("true");
    expect(JSON.parse(localStorage.getItem(ELEMENTS_STORAGE_KEY)!)).toEqual({ "runs.list.cost": true });
    expect(readLocalCopy().elements).toEqual({ "runs.list.cost": true });
    writeLocalCopy(DEFAULT_APPEARANCE);
    expect(localStorage.getItem(ELEMENTS_STORAGE_KEY)).toBeNull();
    localStorage.setItem(ELEMENTS_STORAGE_KEY, "{not json");
    expect(readLocalCopy().elements).toEqual({});
  });
});
