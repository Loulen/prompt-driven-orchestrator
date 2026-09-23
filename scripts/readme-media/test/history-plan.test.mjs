// The mocked history as a plan, before any daemon reads it: volumes, period,
// failure rates, and the cost each synthetic transcript carries (read back with
// the daemon's own formulas: run_cost::line_cost, the copilot constant, pi's
// reported dollars).

import assert from "node:assert/strict";
import { test } from "node:test";
import { MANUAL_PRICES, PROD_CHECK, PROD_CHECK_ROUTES, PROFILES, planHistory } from "../lib/history-plan.mjs";
import { DEMO_PIPELINE } from "../lib/demo-pipeline.mjs";
import { targetPipeline } from "../lib/targets.mjs";

const NOW = new Date("2026-09-22T20:00:00Z");
const REPO = "/tmp/demo/repos/shop-app";
const plan = planHistory({ now: NOW, targetRepo: REPO, triggerId: "trg-demo" });

const median = (values) => {
  const s = [...values].sort((a, b) => a - b);
  const mid = Math.floor(s.length / 2);
  return s.length % 2 ? s[mid] : (s[mid - 1] + s[mid]) / 2;
};

/** Every node execution of the plan with its cost, read back from its file.
 *  What Stats by model count: an execution that names a model. */
function executions() {
  const files = new Map(plan.files.map((f) => [f.path, f.content]));
  const bySession = new Map();
  for (const [file, content] of files) {
    const id = file.match(/([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})(\.jsonl|\/events\.jsonl)$/)?.[1];
    if (id) bySession.set(id, content);
  }
  const out = [];
  const pending = new Map();
  for (const e of plan.events) {
    if (e.kind === "node_started") {
      if (!e.payload.model) continue;
      const content = bySession.get(e.payload.session_id);
      assert.ok(content, `a transcript for session ${e.payload.session_id}`);
      const execution = { runId: e.run_id, node: e.node_id, model: e.payload.model, effort: e.payload.effort, harness: e.payload.harness, usd: costOf(e.payload, content) };
      pending.set(`${e.run_id}/${e.node_id}`, execution);
      out.push(execution);
    } else if ((e.kind === "node_failed" || e.kind === "node_completed") && pending.has(`${e.run_id}/${e.node_id}`)) {
      pending.get(`${e.run_id}/${e.node_id}`).failed = e.kind === "node_failed";
    }
  }
  return out;
}

function costOf({ harness, model }, content) {
  const lines = content.trim().split("\n").map((l) => JSON.parse(l));
  if (harness === "claude") {
    const p = MANUAL_PRICES[model];
    return lines
      .filter((l) => l.type === "assistant")
      .reduce((sum, l) => {
        const u = l.message.usage;
        return sum + (u.input_tokens * p.input + u.output_tokens * p.output + u.cache_creation_input_tokens * p.input * 1.25 + u.cache_read_input_tokens * p.input * 0.1) / 1e6;
      }, 0);
  }
  if (harness === "copilot") {
    const last = lines.filter((l) => l.type === "session.usage_checkpoint").at(-1);
    return (last.data.totalNanoAiu / 1e9) * 0.01;
  }
  return lines.filter((l) => l.type === "message" && l.message.role === "assistant").reduce((sum, l) => sum + l.message.usage.cost.total, 0);
}

const all = executions();
const key = (e) => `${e.model}/${e.effort}`;
const group = (list, by) => list.reduce((m, e) => m.set(by(e), [...(m.get(by(e)) ?? []), e]), new Map());

const prodRuns = new Set(plan.prodChecks.map((p) => p.runId));
const reviewEvents = plan.events.filter((e) => !prodRuns.has(e.run_id));

test("every run the trigger did not fire is the demo pipeline, on its two nodes, with the snapshot of its target", () => {
  const started = reviewEvents.filter((e) => e.kind === "run_started");
  assert.ok(started.length > 150);
  for (const e of started) {
    assert.equal(e.payload.pipeline_id, "implement-review");
    assert.equal(e.payload.triggered_by, undefined);
    assert.deepEqual(e.payload.node_defs, DEMO_PIPELINE.nodeDefs);
    assert.deepEqual(e.payload.edges, DEMO_PIPELINE.edges);
  }
  const nodes = new Set(reviewEvents.filter((e) => e.node_id).map((e) => e.node_id));
  assert.deepEqual([...nodes].sort(), ["implementer", "reviewer"]);
});

test("about 50 executions per model × node (± 30 %), over the last 30 days", () => {
  const byModelNode = group(all, (e) => `${e.model}/${e.node}`);
  assert.equal(byModelNode.size, 8, "4 models × 2 nodes");
  for (const [pair, list] of byModelNode) {
    assert.ok(list.length >= 35 && list.length <= 65, `${pair}: ${list.length} executions`);
  }
  const times = plan.events.map((e) => Date.parse(e.ts));
  assert.ok(Math.min(...times) >= NOW.getTime() - 31 * 86_400_000);
  assert.ok(Math.max(...times) < NOW.getTime());
});

test("median cost per execution: Opus < Fable low ≪ Fable high, GPT ≈ Opus / 2, GLM ≈ Opus / 75", () => {
  const medians = Object.fromEntries([...group(all, key)].map(([k, list]) => [k, median(list.map((e) => e.usd))]));
  const opus = medians["claude-opus-5-5/medium"];
  const ratio = (k) => medians[k] / opus;
  assert.ok(ratio("claude-fable-5-1/low") > 1.03 && ratio("claude-fable-5-1/low") < 1.35, `fable low ${ratio("claude-fable-5-1/low")}`);
  assert.ok(ratio("claude-fable-5-1/high") > 1.8, `fable high ${ratio("claude-fable-5-1/high")}`);
  assert.ok(ratio("gpt-5.6-sol/medium") > 0.4 && ratio("gpt-5.6-sol/medium") < 0.6, `gpt ${ratio("gpt-5.6-sol/medium")}`);
  assert.ok(1 / ratio("z-ai/glm-5.3-flash/medium") > 60 && 1 / ratio("z-ai/glm-5.3-flash/medium") < 90, `glm ${1 / ratio("z-ai/glm-5.3-flash/medium")}`);
});

test("failure rate: about 3 %, a bit more for GPT-5.6 Sol, about 20 % for GLM-5.3 Flash", () => {
  const rate = (model) => {
    const list = all.filter((e) => e.model === model);
    return list.filter((e) => e.failed).length / list.length;
  };
  for (const model of ["claude-opus-5-5", "claude-fable-5-1"]) assert.ok(rate(model) > 0.01 && rate(model) < 0.05, `${model} ${rate(model)}`);
  assert.ok(rate("gpt-5.6-sol") > rate("claude-opus-5-5") && rate("gpt-5.6-sol") < 0.09, `gpt ${rate("gpt-5.6-sol")}`);
  assert.ok(rate("z-ai/glm-5.3-flash") > 0.15 && rate("z-ai/glm-5.3-flash") < 0.25, `glm ${rate("z-ai/glm-5.3-flash")}`);
  assert.ok(plan.events.some((e) => e.kind === "run_failed"), "some runs end failed");
});

test("no run without computable cost: each run carries its manager's (unbilled) transcript", () => {
  const runs = plan.events.filter((e) => e.kind === "run_started").map((e) => e.run_id);
  for (const runId of runs) {
    assert.ok(plan.files.some((f) => f.path.startsWith(".claude/projects/") && f.path.includes(runId) && !f.content.includes('"assistant"')), runId);
  }
});

test("about a hundred fires, each the start of a prod-check run fired by the trigger, about 10 % real incidents", () => {
  assert.ok(plan.fires.length >= 90 && plan.fires.length <= 110, `${plan.fires.length} fires`);
  const started = new Map(plan.events.filter((e) => e.kind === "run_started").map((e) => [e.run_id, e.payload]));
  for (const fire of plan.fires) {
    assert.equal(fire.outcome, "fired");
    assert.equal(fire.guard_exit_code, 0);
    const run = started.get(fire.run_id);
    assert.ok(run, `the run of the fire at ${fire.ts}`);
    assert.equal(run.pipeline_id, "prod-check");
    assert.equal(run.triggered_by, "trg-demo");
    assert.equal(run.input, fire.guard_stdout, "the guard's report is the run's input");
    assert.ok(run.input.startsWith("# Incident — checkout API degraded\n"), "the guard's report, as prod-health-check.sh prints it");
  }
  assert.deepEqual(new Set(plan.prodChecks.map((p) => p.runId)), new Set(plan.fires.map((f) => f.run_id)));
  const share = plan.prodChecks.filter((p) => p.incidentFound).length / plan.prodChecks.length;
  assert.ok(share >= 0.07 && share <= 0.13, `${(share * 100).toFixed(0)} % real incidents`);
});

test("each prod-check run is its target's snapshot and follows the branch of its Incident_found", () => {
  const drawn = targetPipeline("prod-check");
  const byName = Object.fromEntries(drawn.nodes.map((n) => [n.name, n.id]));
  // The branch as drawn: Incident_found = true → Orchestrate Fix, = false → End.
  const branch = (value) => drawn.edges.find((e) => e.source.node === byName["Incident-debugger"] && e.when?.Incident_found?.eq === value).target.node;
  assert.equal(branch(true), byName["Orchestrate Fix"]);
  assert.equal(branch(false), "end");
  assert.deepEqual(PROD_CHECK_ROUTES[true], [byName["Incident-debugger"], byName["Orchestrate Fix"], byName["Notify Slack"]]);
  assert.deepEqual(PROD_CHECK_ROUTES[false], [byName["Incident-debugger"]]);
  for (const { runId, incidentFound } of plan.prodChecks) {
    const events = plan.events.filter((e) => e.run_id === runId);
    const start = events.find((e) => e.kind === "run_started").payload;
    assert.deepEqual(start.node_defs, PROD_CHECK.nodeDefs);
    assert.deepEqual(start.edges, PROD_CHECK.edges);
    const completed = events.filter((e) => e.kind === "node_completed").map((e) => e.node_id);
    assert.deepEqual(completed, PROD_CHECK_ROUTES[incidentFound], runId);
    assert.deepEqual(events.filter((e) => e.kind === "node_started").map((e) => e.node_id), completed);
    assert.equal(events.at(-1).kind, "run_completed");
    // Out of the Stats by model: no model named, no billed turn.
    for (const e of events.filter((ev) => ev.kind === "node_started")) {
      assert.equal(e.payload.model, undefined);
      const transcript = plan.files.find((f) => f.path.endsWith(`${e.payload.session_id}.jsonl`));
      assert.ok(transcript && !transcript.content.includes('"assistant"'), `${runId}/${e.node_id}: an unbilled session`);
    }
  }
});

test("the prod-check runs change nothing in the implement-review history (the Stats)", () => {
  const alone = planHistory({ now: NOW, targetRepo: REPO });
  assert.deepEqual(alone.fires, []);
  assert.deepEqual(reviewEvents, alone.events);
  assert.deepEqual(plan.files.slice(0, alone.files.length), alone.files);
});

test("a few steering messages", () => {
  const steered = plan.files.filter((f) => /Keep prices in cents|Don't touch style.css|empty cart case|formatPrice helper/.test(f.content));
  assert.ok(steered.length >= 5 && steered.length < 60, `${steered.length} steered sessions`);
});

test("the plan is deterministic", () => {
  const again = planHistory({ now: NOW, targetRepo: REPO, triggerId: "trg-demo" });
  assert.deepEqual(again.events, plan.events);
  assert.equal(PROFILES.length, 5);
});
