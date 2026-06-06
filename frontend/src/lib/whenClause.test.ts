import { describe, it, expect } from "vitest";
import { whenToRows, rowsToWhen, OPERATORS } from "./whenClause";

describe("whenToRows", () => {
  it("returns no rows for an empty clause", () => {
    expect(whenToRows(null)).toEqual([]);
    expect(whenToRows(undefined)).toEqual([]);
    expect(whenToRows({})).toEqual([]);
  });

  it("flattens a { field: { op: value } } clause into rows", () => {
    expect(whenToRows({ verdict: { eq: "FAIL" } })).toEqual([
      { field: "verdict", op: "eq", value: "FAIL" },
    ]);
  });

  it("renders a boolean value as the canonical string", () => {
    expect(whenToRows({ is_blocking: { eq: true } })).toEqual([
      { field: "is_blocking", op: "eq", value: "true" },
    ]);
  });

  it("renders an in-list value as JSON", () => {
    expect(whenToRows({ verdict: { in: ["PASS", "APPROVED"] } })).toEqual([
      { field: "verdict", op: "in", value: '["PASS","APPROVED"]' },
    ]);
  });
});

describe("rowsToWhen", () => {
  it("returns null for no rows", () => {
    expect(rowsToWhen([])).toBeNull();
  });

  it("builds a { field: { op: value } } clause", () => {
    expect(rowsToWhen([{ field: "verdict", op: "eq", value: "FAIL" }])).toEqual({
      verdict: { eq: "FAIL" },
    });
  });

  it("coerces a numeric value to a number", () => {
    expect(rowsToWhen([{ field: "iter", op: "gte", value: "3" }])).toEqual({
      iter: { gte: 3 },
    });
  });

  it("coerces canonical booleans for a bool-typed value", () => {
    // The bool toggle writes "true"/"false"; the clause must carry a real
    // boolean, resolving the true / 1 / True ambiguity (#147).
    expect(
      rowsToWhen([{ field: "is_blocking", op: "eq", value: "true", valueType: "bool" }]),
    ).toEqual({ is_blocking: { eq: true } });
    expect(
      rowsToWhen([{ field: "is_blocking", op: "eq", value: "false", valueType: "bool" }]),
    ).toEqual({ is_blocking: { eq: false } });
  });

  it("does not coerce a non-bool field that happens to read 'true' to a boolean", () => {
    // Without a bool type hint, a literal string stays a string — only the
    // numeric coercion applies, and "true" is not numeric.
    expect(rowsToWhen([{ field: "label", op: "eq", value: "true" }])).toEqual({
      label: { eq: "true" },
    });
  });

  it("parses an in-list value", () => {
    expect(rowsToWhen([{ field: "verdict", op: "in", value: "PASS, FAIL" }])).toEqual({
      verdict: { in: ["PASS", "FAIL"] },
    });
  });
});

describe("OPERATORS", () => {
  it("is the ADR-0002 mechanical predicate set", () => {
    expect(OPERATORS).toEqual(["eq", "neq", "lt", "lte", "gt", "gte", "in", "not_in"]);
  });
});
