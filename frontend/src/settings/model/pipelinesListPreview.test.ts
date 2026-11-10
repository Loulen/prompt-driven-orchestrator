import { describe, expect, it } from "vitest";
import type { PipelineListEntry } from "../../shared/generated/PipelineListEntry";
import { pipelinesListPreview } from "./pipelinesListPreview";

const p = (id: string, modified: string | null, changes = 0) =>
  ({ id, name: id, modified, ...(changes > 0 ? { draft: { changes, nodes: 1, participants: [] } } : {}) }) as PipelineListEntry;

describe("the Pipelines list preview", () => {
  it("shows a sample with a version and a draft when the user sees no Pipeline", () => {
    const shown = pipelinesListPreview([]);
    expect(shown.sample).toBe(true);
    expect(shown.pipelines[0].version?.number).toBeGreaterThan(0);
    expect(shown.pipelines[0].draft?.changes).toBeGreaterThan(0);
    expect(shown.caption).toBe("Sample pipeline · you see no pipeline yet");
  });

  it("shows the one Pipeline the user sees", () => {
    expect(pipelinesListPreview([p("a", null)]).pipelines.map((x) => x.id)).toEqual(["a"]);
  });

  it("shows the two edited last, the latest first", () => {
    const shown = pipelinesListPreview([p("old", "2026-01-01T00:00:00Z", 1), p("x", null), p("new", "2026-01-02T00:00:00Z", 2)]);
    expect(shown.pipelines.map((x) => x.id)).toEqual(["new", "old"]);
    expect(shown.caption).toBe("Your pipelines");
  });

  it("adds one with a draft when the most recent has none", () => {
    const shown = pipelinesListPreview([
      p("new", "2026-01-03T00:00:00Z"),
      p("mid", "2026-01-02T00:00:00Z"),
      p("drafted", "2026-01-01T00:00:00Z", 3),
    ]);
    expect(shown.pipelines.map((x) => x.id)).toEqual(["new", "drafted"]);
  });
});
