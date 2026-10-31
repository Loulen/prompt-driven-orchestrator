// The markdown of `make bench-gates` and of `make run-costs`. Pure.

import assert from "node:assert/strict";
import { test } from "node:test";
import { costRows, costSummary, renderCosts } from "../lib/costs.mjs";
import { formatDuration, renderTable } from "../lib/table.mjs";

test("durations read as seconds, minutes, hours", () => {
  assert.equal(formatDuration(42.4), "42 s");
  assert.equal(formatDuration(187), "3 min 07 s");
  assert.equal(formatDuration(3720), "1 h 02 min");
});

test("one row per measure, one column per ref, n/a and skipped as such", () => {
  const benches = [
    {
      ref: "40e82f49",
      sha: "40e82f49aaaa",
      measures: [
        { id: "structural", label: "First structural failure", command: "make test", seconds: 900, outcome: "caught", load: 4.25 },
        { id: "scoped-front", label: "Scoped gate | front", na: "n/a (no `make scoped` at this ref)" },
      ],
    },
    {
      ref: "integration/x",
      sha: "5ab9abe8bbbb",
      measures: [
        { id: "structural", label: "First structural failure", command: "make fast", seconds: 21, outcome: "caught", load: 3 },
        { id: "scoped-front", label: "Scoped gate | front", skipped: "10 GB free, 25 needed" },
      ],
    },
  ];
  const lines = renderTable(benches).split("\n");
  assert.equal(lines[0], "| Measure | `40e82f49` (40e82f49) | `integration/x` (5ab9abe8) |");
  assert.equal(lines[2], "| First structural failure | `make test`: 15 min 00 s (caught, load 4.3) | `make fast`: 21 s (caught, load 3.0) |");
  assert.equal(lines[3], "| Scoped gate \\| front | n/a (no `make scoped` at this ref) | skipped: 10 GB free, 25 needed |");
});

const run = (id, name, start, end, usd, extra = {}) => ({
  run_id: id,
  name,
  status: end ? "archived" : "running",
  started_at: start,
  completed_at: end,
  cost: { usd, partial: false },
  ...extra,
});

const story = {
  run: run("p", "Story", "2026-10-28T10:00:00Z", "2026-10-28T20:00:00Z", 100),
  children: [
    { run: run("a", "T-1", "2026-10-28T10:00:00Z", "2026-10-28T11:00:00Z", 30), children: [] },
    {
      run: run("b", "T-2", "2026-10-28T11:00:00Z", "2026-10-28T14:00:00Z", 50),
      children: [{ run: run("b1", "T-2 retry", "2026-10-28T12:00:00Z", "2026-10-28T13:00:00Z", 20), children: [] }],
    },
  ],
};

test("per ticket: a child Run with its own children; the total is every Run of the tree", () => {
  const s = costSummary(costRows(story, 0));
  assert.equal(s.total, 200);
  assert.equal(s.orchestration, 100);
  assert.equal(s.tickets, 2);
  assert.equal(s.meanUsd, 50); // (30 + 50 + 20) / 2
  assert.equal(s.medianUsd, 50);
  assert.equal(s.meanSeconds, 7200);
  assert.equal(s.partial, false);
});

test("a Run still going counts up to now and makes the total partial", () => {
  const going = { run: run("p", "Story", "2026-10-31T10:00:00Z", undefined, 12), children: [] };
  const rows = costRows(going, Date.parse("2026-10-31T10:30:00Z"));
  assert.equal(rows[0].seconds, 1800);
  const md = renderCosts(rows);
  assert.match(md, /\$12\.00 \(partial\)/);
  assert.match(md, /^Total: \$12\.00 \(partial/m);
});

test("the cost table lists the Run, then its children, then the totals", () => {
  const md = renderCosts(costRows(story, 0)).split("\n");
  assert.equal(md[2], "| `p` | Story (orchestration, its own nodes) | archived | 10 h 00 min | $100.00 |");
  assert.equal(md[3], "| `a` | T-1 | archived | 1 h 00 min | $30.00 |");
  assert.equal(md[5], "| `b1` |   T-2 retry | archived | 1 h 00 min | $20.00 |");
  assert.ok(md.includes("Total: $200.00, of which orchestration $100.00."));
});

test("a Run without a cost source shows no $0, and one never started lasts 0 s", () => {
  const r = { run_id: "x", name: "T", status: "pending", started_at: null, completed_at: null };
  const rows = costRows({ run: r, children: [] }, Date.now());
  assert.equal(rows[0].seconds, 0);
  assert.equal(rows[0].partial, true);
  assert.match(renderCosts(rows), /\| `x` \| T \(orchestration, its own nodes\) \| pending \| 0 s \| - \(no cost\) \|/);
});
