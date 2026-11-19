import { describe, it, expect } from "vitest";
import { defaultRow, importCheckState, type RowState } from "./importRows";
import type { SkillCandidate } from "../../shared/generated/SkillCandidate";

const candidate = (name: string, valid = true): SkillCandidate =>
  ({ path: `skills/${name}`, name, description: "", valid, file_count: 0, status: valid ? "new" : "invalid" }) as SkillCandidate;

const rowsOf = (candidates: SkillCandidate[], checked: string[]): Map<string, RowState> =>
  new Map(candidates.map((c) => [c.path, { ...defaultRow(c), checked: checked.includes(c.name) }]));

describe("importCheckState (CORE-196)", () => {
  const a = candidate("a");
  const b = candidate("b");
  const bad = candidate("bad", false);

  it("is all when every eligible row is checked, invalid rows ignored", () => {
    expect(importCheckState([a, b, bad], rowsOf([a, b, bad], ["a", "b"]), new Set())).toBe("all");
  });

  it("is some when only part of the eligible rows is checked", () => {
    expect(importCheckState([a, b, bad], rowsOf([a, b, bad], ["a"]), new Set())).toBe("some");
  });

  it("is none when no eligible row is checked, done rows ignored", () => {
    expect(importCheckState([a, b], rowsOf([a, b], ["a"]), new Set([a.path]))).toBe("none");
  });

  it("is none when there is no eligible row at all", () => {
    expect(importCheckState([a, bad], rowsOf([a, bad], ["a"]), new Set([a.path]))).toBe("none");
    expect(importCheckState([], new Map(), new Set())).toBe("none");
  });
});
