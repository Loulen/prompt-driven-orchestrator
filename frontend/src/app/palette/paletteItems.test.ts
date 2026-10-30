import { describe, expect, it, vi } from "vitest";
import { NOTHING_REFUSED } from "../../run/model/runGestures";
import { coreItems, selectionItems } from "./paletteItems";
import type { Appearance } from "../../lib/theme";
import type { PaletteSelection } from "./paletteHost";

const node = (status: string): PaletteSelection =>
  ({
    kind: "node",
    run: { id: "r1", status: "awaiting_user" },
    node: { id: "review", name: "Review", type: "agent", status, iter: 2 },
  }) as PaletteSelection;

const labels = (selection: PaletteSelection | null, refused: string[] = []) =>
  selectionItems(selection, { refused: new Set(refused) }, vi.fn(), vi.fn()).map((i) => i.label);

describe("the selection's gestures", () => {
  it("offers a node waiting for a person Mark complete and Stop, then its Run's", () => {
    expect(labels(node("awaiting_user"))).toEqual(["Mark complete", "Stop", "Pause", "Copy run id"]);
    expect(labels(node("failed"))).toEqual(["Retry", "Pause", "Copy run id"]);
  });

  it("leaves out what the caller may not do", () => {
    expect(labels(node("awaiting_user"), ["r1/review mark_node_done", "r1 pause_run"])).toEqual(["Stop", "Copy run id"]);
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
  const commands = (appearance: Appearance, setTheme = vi.fn(), toggleMode = vi.fn()) =>
    coreItems({ host, pipelines: [], openPipeline: vi.fn(), appearance, setTheme, toggleMode }).filter((i) =>
      ["cmd:theme-acogni", "cmd:theme-classic", "cmd:dark-mode"].includes(i.id),
    );

  it("offers Theme: Acogni, Theme: Classic and Dark mode, each checked while it holds", () => {
    const rows = (a: Appearance) => commands(a).map((i) => [i.label, i.checked]);
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
