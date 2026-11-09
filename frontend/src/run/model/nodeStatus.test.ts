import { describe, expect, it } from "vitest";
import type { NodeState } from "../../shared/generated/NodeState";
import { terminalPlaceholder } from "./nodeStatus";

describe("terminalPlaceholder", () => {
  it("says in English that a pending node has not started yet", () => {
    const node = { node_id: "asker", status: "pending", iter: 1 } as NodeState;
    expect(terminalPlaceholder(node)).toBe("Not started yet");
  });
});
