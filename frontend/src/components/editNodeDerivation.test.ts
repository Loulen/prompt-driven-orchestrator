import { describe, it, expect } from "vitest";
import { runReachedEnd } from "./editNodeDerivation";
import type { RunStatus } from "../types";

describe("runReachedEnd", () => {
  it("is true only when the run completed successfully", () => {
    expect(runReachedEnd("completed")).toBe(true);
  });

  it("is false for live and non-success terminal statuses", () => {
    const notReached: RunStatus[] = [
      "running",
      "awaiting_user",
      "paused",
      "failed",
      "halted",
      "archived",
    ];
    for (const status of notReached) {
      expect(runReachedEnd(status)).toBe(false);
    }
  });
});
