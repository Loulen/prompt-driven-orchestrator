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
        "in the run's worktree (e.g. claude --resume) until the run is archived.",
    );
  });

  it("names the node it acts on", () => {
    expect(nodeGestureConfirmation({ gesture: "stop", status: "running", nodeName: "review-2" })?.title).toBe(
      "Stop node review-2?",
    );
  });
});

describe("a Retry asks when it ends a live session or resets downstream nodes (CORE-206)", () => {
  const retry = (status: NodeStatus, downstreamWithArtifacts = 0) =>
    nodeGestureConfirmation({ gesture: "retry", status, nodeName: "implement", downstreamWithArtifacts });

  for (const status of ["running", "awaiting_user"] as const) {
    it(`on a ${status} node with nothing downstream, says it ends its live session`, () => {
      expect(retry(status)).toEqual({
        title: "Retry node implement?",
        consequences: ["Ends its live session"],
        danger: true,
        actionLabel: "Retry",
      });
    });
  }

  it("on a settled node whose downstream holds artifacts, says how many nodes it resets", () => {
    expect(retry("completed", 1)).toEqual({
      title: "Retry node implement?",
      consequences: ["Resets 1 downstream node"],
      danger: true,
      actionLabel: "Retry",
    });
    expect(retry("failed", 3)?.consequences).toEqual(["Resets 3 downstream nodes"]);
  });

  it("both: one line per consequence, the live session first, in the danger tone", () => {
    const c = retry("running", 2);
    expect(c?.consequences).toEqual(["Ends its live session", "Resets 2 downstream nodes"]);
    expect(c?.danger).toBe(true);
    expect(c?.info).toBeUndefined();
  });

  for (const status of ["failed", "stopped", "completed", "interrupted", "stale", "skipped"] as const) {
    it(`on a ${status} node with nothing downstream, goes at once`, () => {
      expect(retry(status)).toBeNull();
      expect(nodeGestureConfirmation({ gesture: "retry", status, nodeName: "n" })).toBeNull();
    });
  }
});
