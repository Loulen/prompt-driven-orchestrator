import { describe, expect, it } from "vitest";
import type { LiveArtifactIO } from "../../shared/generated/LiveArtifactIO";
import { liveDisplayPath, liveUpdatedAgo, newestUpdate } from "./liveArtifacts";

function artifact(name: string, updated_at: string | null): LiveArtifactIO {
  return {
    name,
    port_type: "markdown",
    declared: true,
    path: `worker/iter-1/live/${name}.md`,
    files: [],
    schema_warnings: [],
    updated_at,
  };
}

describe("live artifacts in the Run view (ADR-0089)", () => {
  it("shows the path from the live folder on", () => {
    expect(liveDisplayPath("worker/iter-2/live/progress.md")).toBe("live/progress.md");
    expect(liveDisplayPath("worker/iter-2/live/screens/")).toBe("live/screens/");
    expect(liveDisplayPath("elsewhere.md")).toBe("elsewhere.md");
  });

  it("dates the section by its newest artifact", () => {
    expect(newestUpdate([artifact("a", null)])).toBeNull();
    expect(
      newestUpdate([
        artifact("a", "2026-10-19T10:00:00Z"),
        artifact("b", "2026-10-19T10:05:00Z"),
        artifact("c", null),
      ]),
    ).toBe("2026-10-19T10:05:00Z");
  });

  it("says how long ago, in seconds while it is fresh", () => {
    const now = Date.parse("2026-10-19T10:00:00Z");
    expect(liveUpdatedAgo("2026-10-19T09:59:40Z", now)).toBe("20 s ago");
    expect(liveUpdatedAgo("2026-10-19T09:57:00Z", now)).toBe("3 min ago");
    expect(liveUpdatedAgo("2026-10-19T07:00:00Z", now)).toBe("3 h ago");
    expect(liveUpdatedAgo("2026-10-18T09:00:00Z", now)).toBe("1 day ago");
  });
});
