import { describe, expect, it } from "vitest";
import type { RunListEntry } from "../../shared/generated/RunListEntry";
import { shownNodes } from "./runRowNodes";

const nodes: RunListEntry["nodes"] = [
  { id: "a", status: "completed" },
  { id: "b", status: "running" },
  { id: "c", status: "awaiting_user" },
  { id: "d", status: "pending" },
  { id: "e", status: "failed" },
];

const run = (status: RunListEntry["status"]) => ({ run_id: "r", status, nodes }) as RunListEntry;

describe("the nodes a Run's row draws", () => {
  it("are its nodes while it is not archived", () => {
    expect(shownNodes(run("running"))).toBe(nodes);
  });

  it("stop with a Run archived while they ran or waited, the others as they were", () => {
    expect(shownNodes(run("archived")).map((n) => n.status)).toEqual(["completed", "stopped", "stopped", "pending", "failed"]);
  });

  it("are none without any", () => {
    expect(shownNodes({ run_id: "r", status: "archived" } as RunListEntry)).toEqual([]);
  });
});
