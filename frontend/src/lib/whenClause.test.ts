import { describe, it, expect } from "vitest";
import {
  whenToRows,
  rowsToWhen,
  requalifyWhen,
  renameWhenPorts,
  splitFieldKey,
  fieldKey,
  isPortlessField,
  OPERATORS,
} from "./whenClause";

// Every row carries the output it reads (#843). On a single-port edge that is
// implicit in the YAML, so the helpers below spell the port list out and expect
// the clause to come back unqualified.
const ONE = ["out"];
const TWO = ["out", "design"];

describe("whenToRows", () => {
  it("returns no rows for an empty clause", () => {
    expect(whenToRows(null, ONE)).toEqual([]);
    expect(whenToRows(undefined, ONE)).toEqual([]);
    expect(whenToRows({}, ONE)).toEqual([]);
  });

  it("flattens a { field: { op: value } } clause into rows", () => {
    expect(whenToRows({ verdict: { eq: "FAIL" } }, ONE)).toEqual([
      { port: "out", field: "verdict", op: "eq", value: "FAIL" },
    ]);
  });

  it("renders a boolean value as the canonical string", () => {
    expect(whenToRows({ is_blocking: { eq: true } }, ONE)).toEqual([
      { port: "out", field: "is_blocking", op: "eq", value: "true" },
    ]);
  });

  it("renders an in-list value as JSON", () => {
    expect(whenToRows({ verdict: { in: ["PASS", "APPROVED"] } }, ONE)).toEqual([
      { port: "out", field: "verdict", op: "in", value: '["PASS","APPROVED"]' },
    ]);
  });

  it("reads the output off a qualified key on a multi-port edge (#843)", () => {
    expect(whenToRows({ "design.has_design_work": { eq: true } }, TWO)).toEqual([
      { port: "design", field: "has_design_work", op: "eq", value: "true" },
    ]);
  });

  it("points an unqualified key on a multi-port edge at the default output", () => {
    // Hand-written YAML, or a clause authored before a second port was ticked:
    // `out` is carried, so it is the one the daemon reads.
    expect(whenToRows({ verdict: { eq: "FAIL" } }, TWO)).toEqual([
      { port: "out", field: "verdict", op: "eq", value: "FAIL" },
    ]);
  });

  it("leaves a dotted field name alone when its prefix is not a carried port", () => {
    // `meta.score` is ONE field name here: splitting it would invent a port the
    // edge does not carry and rewrite a clause the user never touched.
    expect(whenToRows({ "meta.score": { gte: 3 } }, TWO)).toEqual([
      { port: "out", field: "meta.score", op: "gte", value: "3" },
    ]);
  });
});

describe("rowsToWhen", () => {
  it("returns null for no rows", () => {
    expect(rowsToWhen([], ONE)).toBeNull();
  });

  it("builds a { field: { op: value } } clause", () => {
    expect(rowsToWhen([{ port: "out", field: "verdict", op: "eq", value: "FAIL" }], ONE)).toEqual({
      verdict: { eq: "FAIL" },
    });
  });

  it("coerces a numeric value to a number", () => {
    expect(rowsToWhen([{ port: "out", field: "iter", op: "gte", value: "3" }], ONE)).toEqual({
      iter: { gte: 3 },
    });
  });

  it("coerces canonical booleans for a bool-typed value", () => {
    // The bool toggle writes "true"/"false"; the clause must carry a real
    // boolean, resolving the true / 1 / True ambiguity (#147).
    expect(
      rowsToWhen(
        [{ port: "out", field: "is_blocking", op: "eq", value: "true", valueType: "bool" }],
        ONE,
      ),
    ).toEqual({ is_blocking: { eq: true } });
    expect(
      rowsToWhen(
        [{ port: "out", field: "is_blocking", op: "eq", value: "false", valueType: "bool" }],
        ONE,
      ),
    ).toEqual({ is_blocking: { eq: false } });
  });

  it("does not coerce a non-bool field that happens to read 'true' to a boolean", () => {
    // Without a bool type hint, a literal string stays a string — only the
    // numeric coercion applies, and "true" is not numeric.
    expect(rowsToWhen([{ port: "out", field: "label", op: "eq", value: "true" }], ONE)).toEqual({
      label: { eq: "true" },
    });
  });

  it("parses an in-list value", () => {
    expect(
      rowsToWhen([{ port: "out", field: "verdict", op: "in", value: "PASS, FAIL" }], ONE),
    ).toEqual({ verdict: { in: ["PASS", "FAIL"] } });
  });

  it("qualifies the key with the output once the edge carries two (#843)", () => {
    expect(
      rowsToWhen([{ port: "design", field: "has_design_work", op: "eq", value: "true", valueType: "bool" }], TWO),
    ).toEqual({ "design.has_design_work": { eq: true } });
  });

  it("never qualifies iter or a pipeline variable", () => {
    expect(
      rowsToWhen(
        [
          { port: "design", field: "iter", op: "gte", value: "3" },
          { port: "design", field: "$budget", op: "lt", value: "5" },
        ],
        TWO,
      ),
    ).toEqual({ iter: { gte: 3 }, $budget: { lt: 5 } });
  });

  it("keeps two rows on the same field but different outputs apart", () => {
    expect(
      rowsToWhen(
        [
          { port: "out", field: "ready", op: "eq", value: "true", valueType: "bool" },
          { port: "design", field: "ready", op: "eq", value: "false", valueType: "bool" },
        ],
        TWO,
      ),
    ).toEqual({ "out.ready": { eq: true }, "design.ready": { eq: false } });
  });

  it("round-trips a single-port clause through rows unchanged", () => {
    // The backward-compatibility contract: a pre-#843 edge re-emits byte for
    // byte, so nothing about its clause may change shape on the way through.
    const when = { verdict: { eq: "FAIL" }, iter: { gte: 2 } };
    expect(rowsToWhen(whenToRows(when, ONE), ONE)).toEqual(when);
  });
});

describe("splitFieldKey / fieldKey", () => {
  it("are inverse on a multi-port edge", () => {
    const { port, field } = splitFieldKey("design.has_design_work", TWO);
    expect(fieldKey({ port, field, op: "eq", value: "" }, TWO)).toBe("design.has_design_work");
  });

  it("leaves the key bare while one port is carried", () => {
    expect(fieldKey({ port: "out", field: "verdict", op: "eq", value: "" }, ONE)).toBe("verdict");
  });
});

describe("isPortlessField", () => {
  it("covers iter, variables and the grammar's any", () => {
    // `any:` holds a LIST of clauses, not a predicate on a field — qualifying it
    // would produce `out.any`, which the daemon's evaluator does not know.
    expect(isPortlessField("iter")).toBe(true);
    expect(isPortlessField("$budget")).toBe(true);
    expect(isPortlessField("any")).toBe(true);
    expect(isPortlessField("verdict")).toBe(false);
  });
});

describe("requalifyWhen", () => {
  it("qualifies every key when a second output is ticked", () => {
    expect(requalifyWhen({ verdict: { eq: "FAIL" } }, ONE, TWO)).toEqual({
      "out.verdict": { eq: "FAIL" },
    });
  });

  it("strips the qualifier when the edge is back to one output", () => {
    expect(requalifyWhen({ "out.verdict": { eq: "FAIL" } }, TWO, ONE)).toEqual({
      verdict: { eq: "FAIL" },
    });
  });

  it("re-points a row whose output was just unticked (ADR-0073 §3)", () => {
    // `design` leaves, so the clause that read it now reads a port still
    // carried rather than dangling on an output the edge no longer delivers.
    expect(
      requalifyWhen({ "design.has_design_work": { eq: true } }, TWO, ["out", "spec"]),
    ).toEqual({ "out.has_design_work": { eq: true } });
  });

  it("preserves an `any:` clause the row editor cannot render", () => {
    // The row model is lossy (it only understands `{field: {op: value}}`), so
    // requalification rewrites KEYS and never round-trips through rows: a port
    // tick must not be able to delete a clause the panel simply doesn't show.
    const when = { any: [{ verdict: { eq: "FAIL" } }, { iter: { gte: 3 } }] };
    expect(requalifyWhen(when, ONE, TWO)).toEqual(when);
  });

  it("returns null for an edge with no condition", () => {
    expect(requalifyWhen(null, ONE, TWO)).toBeNull();
    expect(requalifyWhen({}, ONE, TWO)).toBeNull();
  });
});

describe("renameWhenPorts", () => {
  it("follows a renamed output rather than letting the clause jump elsewhere", () => {
    expect(
      renameWhenPorts({ "design.ready": { eq: true } }, TWO, new Map([["design", "plan"]])),
    ).toEqual({ "plan.ready": { eq: true } });
  });

  it("is a no-op when nothing was renamed", () => {
    const when = { "design.has_design_work": { eq: true } };
    expect(renameWhenPorts(when, TWO, new Map())).toBe(when);
  });

  it("leaves an unqualified single-port clause unqualified", () => {
    expect(renameWhenPorts({ verdict: { eq: "FAIL" } }, ONE, new Map([["out", "result"]]))).toEqual({
      verdict: { eq: "FAIL" },
    });
  });
});

describe("OPERATORS", () => {
  it("is the ADR-0002 mechanical predicate set", () => {
    expect(OPERATORS).toEqual(["eq", "neq", "lt", "lte", "gt", "gte", "in", "not_in"]);
  });
});
