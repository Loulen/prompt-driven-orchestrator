import { describe, expect, it } from "vitest";
import { actorLabel } from "./actor";

describe("actorLabel", () => {
  it.each([
    [{ kind: "local_operator" } as const, "Local operator"],
    [{ kind: "agent", run_id: "20261005-181859-35eef7b", node_id: "dev" } as const, "Agent dev of run -35eef7b"],
    [{ kind: "trigger", trigger_id: "trg-1" } as const, "Trigger trg-1"],
    [{ kind: "identified", namespace: "test", id: "alice" } as const, "alice (test)"],
  ])("names %j", (actor, label) => {
    expect(actorLabel(actor)).toBe(label);
  });
});
