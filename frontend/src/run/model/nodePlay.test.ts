import { describe, expect, it } from "vitest";
import { allowedPlays, nodePlayKind, playQuestion } from "./nodePlay";

describe("the ▶ of a node of a Run (ADR-0086)", () => {
  it("starts an on-demand node the Run never launched, live or finished", () => {
    for (const runStatus of ["running", "completed", "failed", "halted"] as const) {
      expect(nodePlayKind({ onDemand: true, status: "pending", runStatus })).toBe("start");
    }
  });

  it("retries a skipped node, on demand or not", () => {
    expect(nodePlayKind({ onDemand: false, status: "skipped", runStatus: "completed" })).toBe("retry");
    expect(nodePlayKind({ onDemand: true, status: "skipped", runStatus: "running" })).toBe("retry");
  });

  it("offers nothing on an archived Run, on a template, or on a node already launched", () => {
    expect(nodePlayKind({ onDemand: true, status: "pending", runStatus: "archived" })).toBeNull();
    expect(nodePlayKind({ onDemand: false, status: "skipped", runStatus: "archived" })).toBeNull();
    expect(nodePlayKind({ onDemand: true, status: "pending", runStatus: null })).toBeNull();
    expect(nodePlayKind({ onDemand: true, status: "completed", runStatus: "completed" })).toBeNull();
    expect(nodePlayKind({ onDemand: false, status: "pending", runStatus: "running" })).toBeNull();
  });

  it("asks the Operation the daemon checks, and keeps only the allowed ones", () => {
    expect(playQuestion("r1", "review", "start")).toEqual({
      resource: "node",
      verb: { action: "start" },
      target: "r1/review",
    });
    const allowed = allowedPlays(
      [
        { nodeId: "review", kind: "start" },
        { nodeId: "branch", kind: "retry" },
      ],
      [{ decision: "deny", reason: "reader" }, { decision: "allow" }],
    );
    expect([...allowed]).toEqual(["branch:retry"]);
  });
});
