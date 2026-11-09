import { describe, expect, it } from "vitest";
import { isHeartbeatFrame, isVeiled, sessionCameBack } from "./terminalConnection";

describe("the daemon's beat on a terminal socket", () => {
  it("is the heartbeat frame, nothing else", () => {
    expect(isHeartbeatFrame('{"type":"heartbeat"}')).toBe(true);
    expect(isHeartbeatFrame('{"type":"role","role":"solo"}')).toBe(false);
    expect(isHeartbeatFrame("heartbeat")).toBe(false);
    expect(isHeartbeatFrame("null")).toBe(false);
  });
});

describe("the veil of a terminal", () => {
  it("covers a live terminal whose connection fell, unless its node settled", () => {
    expect(isVeiled("live", true, false)).toBe(true);
    expect(isVeiled("live", false, false)).toBe(false);
    expect(isVeiled("live", true, true)).toBe(false);
    expect(isVeiled("frozen", true, false)).toBe(false);
    expect(isVeiled("probing", true, false)).toBe(false);
  });
});

describe("the node's session coming back", () => {
  it("is a live status after one that is not", () => {
    expect(sessionCameBack("interrupted", "running")).toBe(true);
    expect(sessionCameBack("failed", "running")).toBe(true);
    expect(sessionCameBack("stopped", "awaiting_user")).toBe(true);
  });

  it("is not the same session asking, nor a node going down, nor a first status", () => {
    expect(sessionCameBack("running", "awaiting_user")).toBe(false);
    expect(sessionCameBack("running", "interrupted")).toBe(false);
    expect(sessionCameBack(undefined, "running")).toBe(false);
  });
});
