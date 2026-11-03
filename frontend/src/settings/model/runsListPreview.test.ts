import { describe, expect, it } from "vitest";
import type { RunListEntry } from "../../shared/generated/RunListEntry";
import { runsListPreview } from "./runsListPreview";

const run = (run_id: string, started: string | null, over: Partial<RunListEntry> = {}) =>
  ({
    run_id,
    pipeline_name: "p",
    status: "completed",
    stalled: false,
    started_at: started,
    effective_repo: "/r",
    ...over,
  }) as RunListEntry;

const ids = (runs: RunListEntry[]) => runsListPreview(runs).nodes.map((n) => [n.run.run_id, n.depth]);

describe("the preview of the Runs list block", () => {
  it("shows a sample Run, without a Run the user sees", () => {
    const preview = runsListPreview([], Date.parse("2026-01-01T10:00:00Z"));
    expect(preview.sample).toBe(true);
    expect(preview.caption).toBe("Sample run · you see no run yet");
    expect(preview.nodes.map((n) => n.run.name)).toEqual(["Sample run"]);
  });

  it("shows the most recent Run alone when no Run has a child", () => {
    const preview = runsListPreview([run("old", "2026-01-01T09:00:00Z"), run("new", "2026-01-01T10:00:00Z")]);
    expect(preview.sample).toBe(false);
    expect(preview.caption).toBe("Your most recent run");
    expect(preview.nodes.map((n) => n.run.run_id)).toEqual(["new"]);
  });

  it("prefers an active Run to a more recent archived one", () => {
    expect(ids([run("archived", "2026-01-01T12:00:00Z", { status: "archived" }), run("active", "2026-01-01T10:00:00Z")])).toEqual([
      ["active", 0],
    ]);
    expect(ids([run("archived", "2026-01-01T12:00:00Z", { status: "archived" })])).toEqual([["archived", 0]]);
  });

  it("counts a Run not started yet as the most recent", () => {
    expect(ids([run("started", "2026-01-01T10:00:00Z"), run("queued", null)])).toEqual([["queued", 0]]);
  });

  it("adds the most recent parent with its most recent child, the child naming its parent", () => {
    const runs = [
      run("new", "2026-01-01T12:00:00Z"),
      run("parent", "2026-01-01T10:00:00Z"),
      run("child-1", "2026-01-01T10:01:00Z", { parent_run_id: "parent" }),
      run("child-2", "2026-01-01T10:05:00Z", { parent_run_id: "parent" }),
    ];
    const preview = runsListPreview(runs);
    expect(ids(runs)).toEqual([
      ["new", 0],
      ["parent", 0],
      ["child-2", 1],
    ]);
    expect(preview.nodes[2].sectionParentLink).toBe("parent");
    expect(preview.nodes[1].counts.finished).toBe(2);
    expect(preview.caption).toBe("Your most recent run, and a parent with its child");
  });

  it("shows a parent and its child once when the most recent Run is one of them", () => {
    const runs = [run("parent", "2026-01-01T10:00:00Z"), run("child", "2026-01-01T11:00:00Z", { parent_run_id: "parent" })];
    expect(ids(runs)).toEqual([
      ["parent", 0],
      ["child", 1],
    ]);
  });
});
