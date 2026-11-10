import { describe, expect, it, vi } from "vitest";
import { NOTHING_REFUSED } from "../../run/model/runGestures";
import { coreItems, selectionItems } from "./paletteItems";
import type { ThemeChoice } from "../../lib/theme";
import type { PaletteSelection } from "./paletteHost";

const node = (status: string, over: Record<string, unknown> = {}): PaletteSelection =>
  ({
    kind: "node",
    run: { id: "r1", status: "awaiting_user" },
    node: { id: "review", name: "Review", type: "agent", status, iter: 2, ...over },
  }) as PaletteSelection;

const WHY = "You can't change Runs in Project P. Ask an administrator of P.";
const rightsRefusing = (refused: string[]) => ({ refused: new Map(refused.map((k) => [k, WHY] as const)) });
const labels = (selection: PaletteSelection | null, refused: string[] = [], missing: readonly string[] | null = []) =>
  selectionItems(selection, rightsRefusing(refused), vi.fn(), vi.fn(), missing)
    .filter((i) => !i.disabledReason)
    .map((i) => i.label);
const rows = (selection: PaletteSelection, refused: string[] = [], missing: readonly string[] | null = []) =>
  selectionItems(selection, rightsRefusing(refused), vi.fn(), vi.fn(), missing).map((i) => [i.label, i.disabledReason]);

describe("the selection's gestures", () => {
  it("offers a node waiting for a person Approve and Stop, then its Run's", () => {
    expect(labels(node("awaiting_user"))).toEqual(["Approve", "Stop", "Pause", "Copy run id"]);
    expect(labels(node("failed"))).toEqual(["Approve", "Retry", "Pause", "Copy run id"]);
  });

  it("no longer offers « Mark complete »", () => {
    for (const status of ["awaiting_user", "running", "failed"]) {
      const all = selectionItems(node(status, { releasable: true }), NOTHING_REFUSED, vi.fn(), vi.fn()).map((i) => i.label);
      expect(all).not.toContain("Mark complete");
      expect(all.some((l) => /Mark (complete|ready)/.test(l))).toBe(false);
    }
  });

  it("offers Approve exactly where the Inspector does: on what the daemon completes", () => {
    const marks = (status: string) => labels(node(status)).includes("Approve");
    expect(["awaiting_user", "running", "failed"].filter(marks)).toEqual(["awaiting_user", "running", "failed"]);
    // An interrupted node re-drives on Reopen, a stale one on Retry: the daemon refuses to complete them.
    const never = ["interrupted", "stale", "stopped", "completed", "skipped", "pending", "waiting"];
    expect(never.filter(marks)).toEqual([]);
    // A settled Run is reopened to complete the node; a paused or archived one is not.
    const inRun = (status: string) => ({ ...node("failed"), run: { id: "r1", status } }) as PaletteSelection;
    expect(labels(inRun("failed"))).toContain("Approve");
    expect(labels(inRun("halted"))).toContain("Approve");
    expect(labels(inRun("paused"))).not.toContain("Approve");
    expect(labels(inRun("archived"))).not.toContain("Approve");
  });

  it("greys Approve while an output is missing, naming it, and while the outputs are read", () => {
    expect(rows(node("running"), [], ["out", "final"])[0]).toEqual(["Approve", "Missing outputs: out, final"]);
    expect(rows(node("running"), [], null)[0]).toEqual(["Approve", "Reading the outputs…"]);
    expect(rows(node("running"), [], [])[0]).toEqual(["Approve", undefined]);
  });

  it("offers « Auto-approve when ready » on an interactive node whose session lives, missing outputs or not", () => {
    expect(labels(node("running", { releasable: true }), [], ["out"])).toEqual([
      "Auto-approve when ready",
      "Stop",
      "Pause",
      "Copy run id",
    ]);
    expect(labels(node("running", { releasable: false }))).not.toContain("Auto-approve when ready");
    expect(labels(node("running"))).not.toContain("Auto-approve when ready");
    // Off where Approve is off.
    const paused = { ...node("running", { releasable: true }), run: { id: "r1", status: "paused" } } as PaletteSelection;
    expect(selectionItems(paused, NOTHING_REFUSED, vi.fn(), vi.fn()).map((i) => i.label)).not.toContain(
      "Auto-approve when ready",
    );
  });

  it("checks « Auto-approve when ready » once armed, inert", () => {
    const items = selectionItems(node("running", { releasable: true, released: true }), NOTHING_REFUSED, vi.fn(), vi.fn());
    const entry = items.find((i) => i.label === "Auto-approve when ready")!;
    expect(entry.checked).toBe(true);
    expect(entry.disabledReason).toBe("Auto-approve armed");
  });

  it("greys what the caller may not do, with the refusal's sentence (CORE-205), over the missing outputs", () => {
    expect(labels(node("awaiting_user"), ["r1/review mark_node_done", "r1 pause_run"])).toEqual(["Stop", "Copy run id"]);
    expect(rows(node("awaiting_user"), ["r1/review mark_node_done", "r1 pause_run"], ["out"])).toEqual([
      ["Approve", WHY],
      ["Stop", undefined],
      ["Pause", WHY],
      ["Copy run id", undefined],
    ]);
    expect(rows(node("running", { releasable: true }), ["r1/review release_completion"])[1]).toEqual([
      "Auto-approve when ready",
      WHY,
    ]);
  });

  it("offers a failed Run Retry all, a paused one Resume, and nothing for a Trigger", () => {
    const run = (status: string) =>
      ({ kind: "run", run: { id: "r1", name: "Add search", pipeline: "p", status, attributes: {} } }) as PaletteSelection;
    expect(labels(run("failed"))).toEqual(["Retry all", "Copy run id"]);
    expect(labels(run("paused"))).toEqual(["Resume", "Copy run id"]);
    const items = selectionItems(run("failed"), NOTHING_REFUSED, vi.fn(), vi.fn());
    expect(items.every((i) => i.entity?.label === "Add search")).toBe(true);
    expect(labels({ kind: "trigger", trigger: { id: "t", name: "nightly" } } as PaletteSelection)).toEqual([]);
  });

  it("asks before Retry all", () => {
    const confirm = vi.fn();
    const run = { kind: "run", run: { id: "r1", name: null, pipeline: "p", status: "failed", attributes: {} } } as PaletteSelection;
    selectionItems(run, NOTHING_REFUSED, confirm, vi.fn())[0].run();
    expect(confirm).toHaveBeenCalledWith(expect.objectContaining({ title: "Retry all nodes?", actionLabel: "Retry all" }));
  });
});

describe("the Theme and Dark mode commands (ADR-0093)", () => {
  const host = { runs: [], triggers: [], selection: null, openRun: vi.fn(), openTrigger: vi.fn() };
  const commands = (appearance: ThemeChoice, setTheme = vi.fn(), toggleMode = vi.fn()) =>
    coreItems({ host, pipelines: [], openPipeline: vi.fn(), appearance, setTheme, toggleMode, compactNodes: false, toggleCompactNodes: vi.fn() }).filter((i) =>
      ["cmd:theme-acogni", "cmd:theme-classic", "cmd:dark-mode"].includes(i.id),
    );

  it("offers Theme: Acogni, Theme: Classic and Dark mode, each checked while it holds", () => {
    const rows = (a: ThemeChoice) => commands(a).map((i) => [i.label, i.checked]);
    expect(rows({ theme: "acogni", mode: "dark" })).toEqual([
      ["Theme: Acogni", true],
      ["Theme: Classic", false],
      ["Dark mode", true],
    ]);
    expect(rows({ theme: "classic", mode: "light" })).toEqual([
      ["Theme: Acogni", false],
      ["Theme: Classic", true],
      ["Dark mode", false],
    ]);
  });

  it("sets the Theme, and switches the Dark mode", () => {
    const setTheme = vi.fn();
    const toggleMode = vi.fn();
    const [acogni, classic, dark] = commands({ theme: "acogni", mode: "dark" }, setTheme, toggleMode);
    classic.run();
    acogni.run();
    dark.run();
    expect(setTheme.mock.calls).toEqual([["classic"], ["acogni"]]);
    expect(toggleMode).toHaveBeenCalledTimes(1);
  });
});

describe("the Compact nodes command", () => {
  const host = { runs: [], triggers: [], selection: null, openRun: vi.fn(), openTrigger: vi.fn() };
  const row = (compactNodes: boolean, toggleCompactNodes = vi.fn()) =>
    coreItems({
      host,
      pipelines: [],
      openPipeline: vi.fn(),
      appearance: { theme: "acogni", mode: "dark" },
      setTheme: vi.fn(),
      toggleMode: vi.fn(),
      compactNodes,
      toggleCompactNodes,
    }).find((i) => i.id === "cmd:compact-nodes")!;

  it("is checked while Compact nodes is on, and switches it", () => {
    expect([row(false).label, row(false).checked]).toEqual(["Compact nodes", false]);
    expect(row(true).checked).toBe(true);
    const toggle = vi.fn();
    row(false, toggle).run();
    expect(toggle).toHaveBeenCalledTimes(1);
  });
});

describe("the Customize appearance entry", () => {
  const items = (openAppearance?: () => void) =>
    coreItems({
      host: { runs: [], triggers: [], selection: null, openRun: vi.fn(), openTrigger: vi.fn(), openSettings: vi.fn(), openAppearance },
      pipelines: [],
      openPipeline: vi.fn(),
      appearance: { theme: "acogni", mode: "dark" },
      setTheme: vi.fn(),
      toggleMode: vi.fn(),
      compactNodes: false,
      toggleCompactNodes: vi.fn(),
    });

  it("follows Settings and opens Settings › Appearance", () => {
    const open = vi.fn();
    const ids = items(open).map((i) => i.id);
    expect(ids.indexOf("cmd:appearance")).toBe(ids.indexOf("cmd:settings") + 1);
    const entry = items(open).find((i) => i.id === "cmd:appearance")!;
    expect(entry.label).toBe("Customize appearance");
    entry.run();
    expect(open).toHaveBeenCalledTimes(1);
  });

  it("is absent without a shell that opens it", () => {
    expect(items().some((i) => i.id === "cmd:appearance")).toBe(false);
  });
});
