import { describe, expect, it } from "vitest";
import { fmtAge, fmtCost, fmtEdited, fmtSpan, fmtUntil, spanOf } from "./format";

const NOW = Date.parse("2026-10-28T12:00:00Z");

describe("the figures of a row", () => {
  it("say a duration in its largest unit", () => {
    expect(fmtSpan(42_000)).toBe("42s");
    expect(fmtSpan(14 * 60_000)).toBe("14m");
    expect(fmtSpan(75 * 60_000)).toBe("1h15");
    expect(fmtSpan(3 * 86_400_000)).toBe("3d");
  });

  it("measure a span to its end, or to now while it runs", () => {
    expect(spanOf("2026-10-28T11:00:00Z", "2026-10-28T11:30:00Z", NOW)).toBe(30 * 60_000);
    expect(spanOf("2026-10-28T11:00:00Z", null, NOW)).toBe(60 * 60_000);
    expect(spanOf(null, null, NOW)).toBeNull();
  });

  it("say a cost in dollars, a date in day and month", () => {
    expect(fmtCost(0.4141)).toBe("$0.41");
    expect(fmtEdited("2026-10-25T10:00:00Z")).toBe("25 Oct");
    expect(fmtEdited(null)).toBe("");
  });

  it("count the time to a fire and since one", () => {
    expect(fmtUntil("2026-10-28T12:05:00Z", NOW)).toBe("5m");
    expect(fmtUntil("2026-10-29T04:00:00Z", NOW)).toBe("16h");
    expect(fmtUntil("2026-10-28T11:00:00Z", NOW)).toBe("now");
    expect(fmtAge("2026-10-25T12:00:00Z", NOW)).toBe("3d");
  });
});
