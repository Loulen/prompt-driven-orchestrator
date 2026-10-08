import { describe, expect, it } from "vitest";
import { isHeartbeatFrame, isVeiled } from "./terminalConnection";

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
