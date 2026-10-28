import { describe, expect, it, vi } from "vitest";
import { NOTHING_REFUSED } from "../../run/model/runGestures";
import { selectionItems } from "./paletteItems";
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
