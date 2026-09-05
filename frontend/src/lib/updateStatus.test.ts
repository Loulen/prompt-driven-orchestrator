import { describe, it, expect } from "vitest";
import { newerAvailable, relativeTime, upToDate } from "./updateStatus";
import type { UpdateStatus } from "../types";

function status(overrides: Partial<UpdateStatus> = {}): UpdateStatus {
  return {
    installed_version: "1.58.1",
    latest_version: "1.59.0",
    newer_available: true,
    checked_at: "2026-09-05T08:00:00Z",
    source: "GitHub Releases",
    source_url: "https://example.invalid/latest",
    check_enabled: true,
    install_method: "script",
    manual_command: "curl … | sh",
    supervision: "none",
    reason: null,
    last_error: null,
    active_runs: 0,
    can_apply: true,
    apply_blocked_reason: null,
    last_attempt: null,
    ...overrides,
  };
}

describe("updateStatus helpers (#697)", () => {
  it("newerAvailable requires the check on AND a known newer version", () => {
    expect(newerAvailable(status())).toBe(true);
    expect(newerAvailable(status({ check_enabled: false }))).toBe(false);
    expect(newerAvailable(status({ latest_version: null, newer_available: false }))).toBe(false);
    expect(newerAvailable(status({ newer_available: false }))).toBe(false);
    expect(newerAvailable(null)).toBe(false);
  });

  it("upToDate is only claimed when latest is known and equal — never when off or unknown", () => {
    expect(upToDate(status({ latest_version: "1.58.1", newer_available: false }))).toBe(true);
    expect(upToDate(status())).toBe(false);
    expect(upToDate(status({ latest_version: null, newer_available: false }))).toBe(false);
    expect(
      upToDate(status({ check_enabled: false, latest_version: "1.58.1", newer_available: false })),
    ).toBe(false);
  });

  it("relativeTime rounds to the coarsest useful unit", () => {
    const now = Date.parse("2026-09-05T12:00:00Z");
    expect(relativeTime("2026-09-05T11:59:40Z", now)).toBe("just now");
    expect(relativeTime("2026-09-05T11:53:00Z", now)).toBe("7 min ago");
    expect(relativeTime("2026-09-05T09:53:00Z", now)).toBe("2 h ago");
    expect(relativeTime("2026-09-01T12:00:00Z", now)).toBe("4 d ago");
  });
});
