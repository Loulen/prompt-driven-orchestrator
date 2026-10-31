// The measures of `make bench-gates` for a ref, from its Makefile. Pure.

import assert from "node:assert/strict";
import { test } from "node:test";
import { benchMeasures, makeTargets, OVERSIZED_LINES, oversizedSource } from "../lib/plan.mjs";

const before = "SHELL := bash\n.PHONY: build test check\nPORT := 6172\nbuild:\n\tcargo build\ntest:\n\tcargo nextest run\ncheck types:\n\tcargo check\n";
const after = `${before}fast:\n\tcargo fmt --check\nscoped: $(if $(DRY_RUN),,fast)\n\tnode s.mjs\ngate:\n\tnode g.mjs\n`;

test("the targets of a Makefile, never its variables nor its special targets", () => {
  assert.deepEqual([...makeTargets(before)].sort(), ["build", "check", "test", "types"]);
  assert.ok(makeTargets(after).has("scoped"));
  assert.ok(!makeTargets(after).has("PORT"));
  assert.ok(!makeTargets(after).has("SHELL"));
});

test("a ref before the gates: structural failure through `make test`, no scoped gate, `make test` as the suite", () => {
  const m = benchMeasures({ targets: makeTargets(before), sccache: true });
  const byId = Object.fromEntries(m.map((x) => [x.id, x]));
  assert.equal(byId.structural.command, "make test");
  assert.equal(byId.structural.expect, "fail");
  assert.match(byId["scoped-front"].na, /^n\/a/);
  assert.equal(byId["full-suite"].command, "make test");
  assert.equal(byId["warm-build"].na, undefined);
});

test("a ref with the gates: `make fast` catches the structural failure, `make gate` is the suite", () => {
  const m = benchMeasures({ targets: makeTargets(after), sccache: true });
  const byId = Object.fromEntries(m.map((x) => [x.id, x]));
  assert.equal(byId.structural.command, "make fast");
  assert.equal(byId["scoped-front"].command, "make scoped BASE=HEAD");
  assert.equal(byId["scoped-front"].na, undefined);
  assert.equal(byId["full-suite"].command, "make gate");
  assert.deepEqual(
    m.map((x) => x.id),
    ["frontend-deps", "structural", "cold-build", "warm-build", "scoped-front", "full-suite"],
  );
});

test("without sccache, the warm build is « n/a (sccache absent) » and the cold build is not wrapped", () => {
  const m = benchMeasures({ targets: makeTargets(after), sccache: false });
  assert.equal(m.find((x) => x.id === "warm-build").na, "n/a (sccache absent)");
  assert.equal(m.find((x) => x.id === "cold-build").wrapped, false);
});

test("the injected file is one line over the limit, and every measure that dirties the tree cleans it", () => {
  assert.equal(oversizedSource().trimEnd().split("\n").length, OVERSIZED_LINES);
  assert.equal(OVERSIZED_LINES, 401);
  for (const m of benchMeasures({ targets: makeTargets(after), sccache: true })) {
    if (m.setup && !m.setup.startsWith("cargo clean")) assert.ok(m.cleanup, `${m.id} has no cleanup`);
  }
});
