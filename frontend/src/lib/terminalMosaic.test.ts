import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import {
  addPane,
  canAddPane,
  hasRestorable,
  LAST_MOSAIC_KEY,
  loadLastMosaic,
  MAX_PANES,
  mosaicRows,
  paneTitle,
  removePane,
  restoreMosaic,
  saveLastMosaic,
  type TerminalRef,
} from "./terminalMosaic";

function node(runId: string, nodeId: string, iter = 1, runName: string | null = `Run ${runId}`): TerminalRef {
  return {
    session: `pdo-${runId}-${nodeId}-iter-${iter}`,
    kind: "node",
    runId,
    runName,
    subject: nodeId,
    paneSource: { runId, nodeId, iter },
  };
}

function manager(runId: string): TerminalRef {
  return { session: `pdo-mgr-${runId}`, kind: "manager", runId, runName: `Run ${runId}`, subject: "Manager" };
}

/** A second browser: a storage of its own. */
function memoryStorage(): Storage {
  const data = new Map<string, string>();
  return {
    get length() {
      return data.size;
    },
    clear: () => data.clear(),
    getItem: (k) => data.get(k) ?? null,
    key: (i) => [...data.keys()][i] ?? null,
    removeItem: (k) => void data.delete(k),
    setItem: (k, v) => void data.set(k, String(v)),
  };
}

const primary = node("r1", "design");

describe("the Mosaïque's panes (#1040)", () => {
  it("adds a session as a pane and focuses it", () => {
    const b = node("r2", "implement");
    const out = addPane(primary.session, [], b);
    expect(out).toEqual({ extras: [b], focus: b.session, added: true });
  });

  it("never opens a session twice: an already shown one is focused instead", () => {
    const b = node("r2", "implement");
    const out = addPane(primary.session, [b], { ...b });
    expect(out.added).toBe(false);
    expect(out.extras).toEqual([b]);
    expect(out.focus).toBe(b.session);
  });

  it("treats the primary terminal as shown", () => {
    const out = addPane(primary.session, [], primary);
    expect(out).toEqual({ extras: [], focus: primary.session, added: false });
  });

  it("holds four panes at most, the primary included", () => {
    let extras: TerminalRef[] = [];
    for (const id of ["a", "b", "c"]) extras = addPane(primary.session, extras, node("r2", id)).extras;
    expect(extras).toHaveLength(MAX_PANES - 1);
    expect(canAddPane(extras)).toBe(false);
    const out = addPane(primary.session, extras, node("r2", "d"));
    expect(out).toEqual({ extras, focus: null, added: false });
    // Still focusable when already shown, even full.
    expect(addPane(primary.session, extras, node("r2", "b")).focus).toBe(node("r2", "b").session);
  });

  it("removes a pane by its session, the others keep their order", () => {
    const [a, b, c] = [node("r2", "a"), manager("r3"), node("r4", "c")];
    expect(removePane([a, b, c], b.session)).toEqual([a, c]);
    expect(removePane([a], "unknown")).toEqual([a]);
  });

  it("lays the panes out side by side, in columns, then in a 2×2 grid", () => {
    expect(mosaicRows([1])).toEqual([[1]]);
    expect(mosaicRows([1, 2])).toEqual([[1, 2]]);
    expect(mosaicRows([1, 2, 3])).toEqual([[1, 2, 3]]);
    expect(mosaicRows([1, 2, 3, 4])).toEqual([
      [1, 2],
      [3, 4],
    ]);
  });

  it("titles a pane « Run › subject »", () => {
    expect(paneTitle(node("r2", "implement", 1, "US #7"))).toBe("US #7 › implement");
    expect(paneTitle(manager("r3"))).toBe("Run r3 › Manager");
    expect(paneTitle(node("r2", "implement", 1, null))).toBe("implement");
  });
});

describe("the poste's last mosaic (#1040)", () => {
  beforeEach(() => localStorage.clear());
  afterEach(() => vi.restoreAllMocks());

  it("is kept in this browser's storage and read back after a reload", () => {
    const composition = [primary, node("r2", "implement"), manager("r3")];
    saveLastMosaic(composition);
    expect(localStorage.getItem(LAST_MOSAIC_KEY)).not.toBeNull();
    expect(loadLastMosaic()).toEqual(composition);
  });

  it("belongs to the poste: another browser has its own, or none", () => {
    const other = memoryStorage();
    saveLastMosaic([primary, node("r2", "implement")]);
    expect(loadLastMosaic(other)).toBeNull();
    saveLastMosaic([node("r9", "x"), manager("r9")], other);
    expect(loadLastMosaic()).toEqual([primary, node("r2", "implement")]);
    expect(loadLastMosaic(other)).toEqual([node("r9", "x"), manager("r9")]);
  });

  it("is not overwritten by a lone terminal", () => {
    saveLastMosaic([primary, node("r2", "implement")]);
    saveLastMosaic([primary]);
    expect(loadLastMosaic()).toEqual([primary, node("r2", "implement")]);
  });

  it("drops what it cannot read", () => {
    localStorage.setItem(LAST_MOSAIC_KEY, "{not json");
    expect(loadLastMosaic()).toBeNull();
    localStorage.setItem(LAST_MOSAIC_KEY, JSON.stringify([primary, { session: 3 }]));
    expect(loadLastMosaic()).toBeNull();
    localStorage.setItem(LAST_MOSAIC_KEY, JSON.stringify([primary, { session: "x" }, manager("r3")]));
    expect(loadLastMosaic()).toEqual([primary, manager("r3")]);
  });

  it("never throws when storage is unavailable", () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("quota");
    });
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("disabled");
    });
    expect(() => saveLastMosaic([primary, manager("r3")])).not.toThrow();
    expect(loadLastMosaic()).toBeNull();
    expect(loadLastMosaic(null)).toBeNull();
  });

  it("offers a restore only when it holds another terminal than the one shown", () => {
    expect(hasRestorable(primary.session, null)).toBe(false);
    expect(hasRestorable(primary.session, [primary, manager("r3")])).toBe(true);
    expect(hasRestorable(primary.session, [primary, primary])).toBe(false);
  });

  it("restores around the primary, skipping the sessions no longer live", () => {
    const [b, c, d] = [node("r2", "implement"), manager("r3"), node("r4", "review")];
    const live = [c, d, node("r5", "other")];
    expect(restoreMosaic(primary.session, [primary, b, c, d], live)).toEqual([c, d]);
  });

  it("restores the old primary as a pane when expanding another node", () => {
    const other = node("r1", "plan");
    const b = node("r2", "implement");
    expect(restoreMosaic(other.session, [primary, b], [primary, b])).toEqual([primary, b]);
  });

  it("caps a restore at four panes and takes the live identity", () => {
    const other = node("r8", "x");
    const panes = [primary, node("r2", "a"), node("r3", "b"), node("r4", "c")];
    const live = panes.map((p) => ({ ...p, runName: "Renamed" }));
    const restored = restoreMosaic(other.session, panes, live);
    expect(restored).toHaveLength(MAX_PANES - 1);
    expect(restored.every((p) => p.runName === "Renamed")).toBe(true);
  });
});
