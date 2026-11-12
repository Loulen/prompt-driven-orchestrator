import { describe, expect, it } from "vitest";
import { isWatching, parseRoleFrame, readOnlyHint, readOnlyOf, SOLO } from "./terminalRole";

const WHY = "You can't attach this session here. Ask an administrator of the test extension.";

describe("terminal role frames", () => {
  it("reads the roles, and a read-only connection's sentence", () => {
    expect(parseRoleFrame(JSON.stringify({ type: "role", role: "solo" }))).toEqual(SOLO);
    expect(parseRoleFrame(JSON.stringify({ type: "role", role: "solo", read_only: WHY }))).toEqual({
      role: "solo",
      readOnly: WHY,
    });
    expect(
      parseRoleFrame(JSON.stringify({ type: "role", role: "spectator", pilot: { cols: 80, rows: 24 }, read_only: WHY })),
    ).toEqual({ role: "spectator", pilot: { cols: 80, rows: 24 }, readOnly: WHY });
    expect(
      parseRoleFrame(JSON.stringify({ type: "role", role: "spectator", pilot: { cols: 80, rows: 24 } })),
    ).toEqual({ role: "spectator", pilot: { cols: 80, rows: 24 } });
    expect(parseRoleFrame(JSON.stringify({ type: "role", role: "solo", read_only: "" }))).toEqual(SOLO);
  });

  it("a read-only connection watches whatever its role", () => {
    expect(isWatching(SOLO)).toBe(false);
    expect(isWatching({ role: "pilot", spectators: 1 })).toBe(false);
    expect(isWatching({ role: "spectator", pilot: { cols: 1, rows: 1 } })).toBe(true);
    expect(isWatching({ role: "solo", readOnly: WHY })).toBe(true);
    expect(readOnlyOf({ role: "solo", readOnly: WHY })).toBe(WHY);
    expect(readOnlyOf({ role: "spectator", pilot: { cols: 1, rows: 1 } })).toBeUndefined();
  });

  it("the screen's hint says it can't type, then why", () => {
    expect(readOnlyHint(WHY)).toBe(`Read-only — you can't type in this terminal. ${WHY}`);
  });
});
