import { describe, it, expect } from "vitest";
import { isLiveRun, isTerminalRun, type RunStatus } from "./types";

// The full 8-status universe — kept exhaustive so a new RunStatus variant forces
// a decision here (#316).
const ALL: RunStatus[] = [
  "running",
  "awaiting_user",
  "completed",
  "failed",
  "skipped",
  "halted",
  "paused",
  "archived",
];

const LIVE: RunStatus[] = ["running", "awaiting_user", "paused"];

describe("isLiveRun / isTerminalRun", () => {
  it("isTerminalRun is the exact complement of isLiveRun for every status", () => {
    for (const s of ALL) {
      expect(isTerminalRun(s)).toBe(!isLiveRun(s));
    }
  });

  it("live statuses are {running, awaiting_user, paused}", () => {
    for (const s of ALL) {
      expect(isLiveRun(s)).toBe(LIVE.includes(s));
    }
  });

  it("terminal statuses are {completed, failed, skipped, halted, archived} — INCLUDING archived", () => {
    const terminal = ALL.filter(isTerminalRun);
    expect(new Set(terminal)).toEqual(
      new Set(["completed", "failed", "skipped", "halted", "archived"]),
    );
    // Load-bearing gotcha: `isTerminalRun("archived")` is true — the shell
    // action must exclude archived on top of the terminal check.
    expect(isTerminalRun("archived")).toBe(true);
  });
});
