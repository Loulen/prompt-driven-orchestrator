import { describe, expect, it } from "vitest";
import type { NodeStatus } from "../../shared/generated/NodeStatus";
import { STOP_INFO, nodeGestureConfirmation, type ConfirmedGesture } from "./nodeGestureConfirmation";

const ALL_STATUSES: NodeStatus[] = [
  "pending",
  "waiting",
  "running",
  "awaiting_user",
  "stale",
  "completed",
  "failed",
  "stopped",
  "interrupted",
  "skipped",
];

describe("a node gesture that destroys something is confirmed first (CORE-206)", () => {
  const table: {
    gesture: ConfirmedGesture;
    statuses: NodeStatus[];
    title: string;
    consequences: string[];
    actionLabel: string;
    info: boolean;
  }[] = [
    {
      gesture: "stop",
      statuses: ["running", "awaiting_user", "stale"],
      title: "Stop node implement?",
      consequences: ["Ends its agent session"],
      actionLabel: "Stop",
      info: true,
    },
    {
      gesture: "restart",
      statuses: ["running", "stale"],
      title: "Restart node implement?",
      consequences: ["Ends its session, restarts clean"],
      actionLabel: "Restart",
      info: false,
    },
    {
      gesture: "kill_stale",
      statuses: ["stale"],
      title: "Stop stale node implement?",
      consequences: ["Marks it failed"],
      actionLabel: "Stop",
      info: false,
    },
  ];

  for (const row of table) {
    for (const status of row.statuses) {
      it(`${row.gesture} on a ${status} node asks, in the danger tone`, () => {
        const c = nodeGestureConfirmation({ gesture: row.gesture, status, nodeName: "implement" });
        expect(c).toEqual({
          title: row.title,
          consequences: row.consequences,
          danger: true,
          actionLabel: row.actionLabel,
          ...(row.info ? { info: STOP_INFO } : {}),
        });
      });
    }
  }

  it("Stop, Restart clean and the stale Stop ask whatever the node's status", () => {
    for (const gesture of ["stop", "restart", "kill_stale"] as const) {
      for (const status of ALL_STATUSES) {
        expect(nodeGestureConfirmation({ gesture, status, nodeName: "n" })).not.toBeNull();
      }
    }
  });

  it("the Stop's (i) says Retry starts afresh and how to resume the conversation by hand", () => {
    expect(STOP_INFO.label).toBe("Ends this conversation");
    expect(STOP_INFO.description).toBe(
      "Retry starts a fresh agent. Depending on the harness, you can still resume this conversation by hand " +
        "in the run's worktree (e.g. `claude --resume`) until the run is archived.",
    );
  });

  it("names the node it acts on", () => {
    expect(nodeGestureConfirmation({ gesture: "stop", status: "running", nodeName: "review-2" })?.title).toBe(
      "Stop node review-2?",
    );
  });
});
