// The mocked history of the demo instance (ADR-0074 §3), as a PURE plan: event
// rows for the event log, synthetic transcript files for the demo `HOME`, and
// trigger fires. `history-write.mjs` puts the plan on disk; the tests read the
// plan directly.
//
// The recipe is the one of the Stats-by-model tests
// (crates/pdo-daemon/tests/stats_cost_by_model.rs): a `node_started` freezes the
// harness, model, effort and session id; the cost is read back from a transcript
// found by that identity under the (redirected) home — claude derived from
// tokens × the price table, copilot reported in nano-AIU, pi reported in USD.
//
// Everything lives on ONE pipeline, the demo pipeline `implement-review`
// (design: « one pipeline only »), on its two nodes `implementer` and `reviewer`.

import path from "node:path";
import { createRng, normalQuantiles } from "./rng.mjs";
import { DEMO_PIPELINE } from "./demo-pipeline.mjs";

/**
 * The four models of the README, and what each costs, lasts and fails.
 * `costMult` is relative to Opus 5.5 (medium); the targets come from the
 * grilling (Q25): Fable 5.1 low slightly dearer than Opus, Fable 5.1 high much
 * dearer, GPT-5.6 Sol about half of Opus, GLM-5.3 Flash about Opus / 75.
 * `successes` is per node; `failRate` is over every attempt (failed ones add
 * executions on top of the successes).
 */
export const PROFILES = [
  { id: "opus-medium", harness: "claude", model: "claude-opus-5-5", effort: "medium", costMult: 1, failRate: 0.03, speed: 1, ctxMult: 1, successes: 50 },
  { id: "fable-low", harness: "claude", model: "claude-fable-5-1", effort: "low", costMult: 1.15, failRate: 0.03, speed: 0.85, ctxMult: 0.8, successes: 26 },
  { id: "fable-high", harness: "claude", model: "claude-fable-5-1", effort: "high", costMult: 2.3, failRate: 0.03, speed: 1.7, ctxMult: 1.25, successes: 26 },
  { id: "gpt-medium", harness: "copilot", model: "gpt-5.6-sol", effort: "medium", costMult: 0.5, failRate: 0.06, speed: 1.15, ctxMult: 0.9, successes: 50 },
  // Fewer successes: its failed attempts (and their retries) bring it back to ~50.
  { id: "glm-medium", harness: "pi", model: "z-ai/glm-5.3-flash", effort: "medium", costMult: 1 / 75, failRate: 0.2, speed: 0.75, ctxMult: 0.7, successes: 44 },
];

/** Per-node baselines at Opus 5.5 (medium): median $/execution, minutes, context peak. */
export const NODE_BASELINES = {
  implementer: { usd: 1.8, minutes: 9, context: 110_000 },
  reviewer: { usd: 1.1, minutes: 3.5, context: 55_000 },
};

/** Per-MTok list prices the manual price table gives the two Claude models the
 *  compiled table does not price (written to `~/.pdo/prices/models.yaml`). */
export const MANUAL_PRICES = {
  "claude-opus-5-5": { input: 5, output: 25 },
  "claude-fable-5-1": { input: 10, output: 50 },
};

/** The published copilot constant: 1 AIU = $0.01, counted in nano-AIU. */
const NANO_AIU_PER_USD = 1e11;
const COST_SPREAD = 0.3;
const LOOP_SHARE = 0.1;
const STEERING_SHARE = 0.06;

const TASKS = [
  "Add a cart badge to the header",
  "Show the cart total in the header",
  "Persist the cart in localStorage",
  "Add a remove button to cart lines",
  "Sort products by price",
  "Add a search box above the grid",
  "Show a toast when an item is added",
  "Format prices with Intl.NumberFormat",
  "Add quantity steppers to cart lines",
  "Close the cart panel on Escape",
  "Add a free-shipping progress bar",
  "Lazy-load product images",
  "Add a dark theme toggle",
  "Group cart lines by product",
  "Disable Add to cart when out of stock",
  "Add an empty-cart message",
  "Add keyboard focus styles to cards",
  "Show a discount badge on sale items",
  "Add a product detail modal",
  "Validate the checkout email field",
  "Add a Clear cart button",
  "Animate the cart badge on change",
  "Add a sticky header",
  "Add a skeleton while products load",
  "Round totals to the cent everywhere",
];

const INCIDENT_REPORT = [
  "# Incident — checkout API degraded",
  "- /api/checkout p95 = 4.2 s (budget 800 ms)",
  "- 5xx rate 12 % since 21:04",
  "- last deploy: a1f3c9e \"cart: batch price lookups\"",
].join("\n");

const STEERING_MESSAGES = [
  "Keep prices in cents end to end, please.",
  "Don't touch style.css for this one.",
  "Also cover the empty cart case.",
  "Use the existing formatPrice helper.",
];

/**
 * Build the whole history. `now` is a Date (the last run ends before it),
 * `targetRepo` the demo repository path (transcripts are keyed by the worktree
 * under it), `triggerId` the demo trigger the incident runs were fired by.
 */
export function planHistory({ now, targetRepo, triggerId = null, seed = 852, days = 30 }) {
  const rng = createRng(seed);
  const nodeIds = ["implementer", "reviewer"];

  // One deck of successful executions per node: every profile `successes`
  // times, each with a cost/duration/context draw from shuffled quantiles.
  const decks = {};
  for (const nodeId of nodeIds) {
    const deck = [];
    for (const profile of PROFILES) {
      const quantiles = rng.shuffle(normalQuantiles(profile.successes));
      for (const z of quantiles) deck.push({ profile, z });
    }
    decks[nodeId] = rng.shuffle(deck);
  }
  const runCount = decks.implementer.length;

  // Failed attempts: an exact count per profile, split across the two nodes.
  // Half of the implementer's failures end their Run; every other failure is
  // retried in place (a restart), and the retry succeeds.
  const failures = [];
  for (const profile of PROFILES) {
    const attempts = profile.successes * nodeIds.length;
    const total = Math.round((attempts * profile.failRate) / (1 - profile.failRate));
    for (let k = 0; k < total; k++) failures.push({ profile, nodeId: nodeIds[k % 2], standalone: false });
  }
  let implementerFailures = 0;
  for (const failure of failures) {
    if (failure.nodeId !== "implementer") continue;
    failure.standalone = implementerFailures % 2 === 0;
    implementerFailures++;
  }

  const runs = [];
  for (let i = 0; i < runCount; i++) {
    runs.push({
      laps: [{ implementer: [decks.implementer[i]], reviewer: [decks.reviewer[i]] }],
      failedAttempts: { implementer: [], reviewer: [] },
      status: "completed",
    });
  }
  for (const failure of failures.filter((f) => !f.standalone)) {
    const candidates = runs
      .map((run, index) => ({ run, index }))
      .filter(({ run }) => run.laps[0][failure.nodeId][0].profile === failure.profile);
    const { run } = candidates.length > 0 ? rng.pick(candidates) : { run: rng.pick(runs) };
    run.failedAttempts[failure.nodeId].push({ profile: failure.profile, z: -0.5 });
  }
  // A few Runs loop once (reviewer said fail, implementer goes again).
  for (const run of runs) {
    if (rng.next() < LOOP_SHARE) {
      run.laps.push({
        implementer: [{ profile: run.laps[0].implementer[0].profile, z: rng.next() - 0.5 }],
        reviewer: [{ profile: run.laps[0].reviewer[0].profile, z: rng.next() - 0.5 }],
      });
    }
  }
  for (const failure of failures.filter((f) => f.standalone)) {
    runs.push({
      laps: [{ implementer: [], reviewer: [] }],
      failedAttempts: { implementer: [{ profile: failure.profile, z: 0 }], reviewer: [] },
      status: "failed",
    });
  }
  rng.shuffle(runs);

  // Spread the Runs over the last `days` days, working hours, oldest first.
  const end = now.getTime() - 60 * 60 * 1000;
  const start = end - days * 24 * 60 * 60 * 1000;
  const starts = runs
    .map(() => {
      const day = Math.floor(rng.next() * days);
      const hour = 8 + rng.next() * 13;
      return start + day * 86_400_000 + hour * 3_600_000;
    })
    .sort((a, b) => a - b);

  const events = [];
  const files = [];
  const firedRuns = [];
  const nodeDefs = DEMO_PIPELINE.nodeDefs;
  let messageCounter = 0;
  const nextMessageId = () => `msg_demo_${String(++messageCounter).padStart(6, "0")}`;

  runs.forEach((run, index) => {
    const runStart = Math.min(starts[index], end - 60 * 60 * 1000);
    const runId = runIdAt(new Date(runStart), rng);
    const worktree = path.join(targetRepo, ".pdo", "runs", runId, "worktree");
    const triggered = triggerId && index % 17 === 5;
    const task = triggered ? "Incident: checkout API degraded" : TASKS[index % TASKS.length];
    let t = runStart;
    const at = () => new Date(t).toISOString();

    const runPayload = {
      pipeline_id: DEMO_PIPELINE.id,
      pipeline_name: DEMO_PIPELINE.name,
      name: task,
      input: triggered ? INCIDENT_REPORT : `${task}.`,
      target_repo: targetRepo,
      harness: "claude",
      sandbox: "off",
      node_defs: nodeDefs,
      edges: DEMO_PIPELINE.edges,
    };
    if (triggered) {
      runPayload.triggered_by = triggerId;
      firedRuns.push({ runId, ts: at() });
    }
    events.push({ run_id: runId, ts: at(), kind: "run_started", node_id: null, iter: null, payload: runPayload });
    t += 4_000;

    // The run's manager session: an infrastructure transcript with no billed
    // turn, so the infrastructure slice is readable ($0) and no Run shows up
    // as « without computable cost ».
    files.push({
      path: path.join(".claude", "projects", encodeClaudeDir(worktree), `${rng.uuid()}.jsonl`),
      content: `${JSON.stringify({ type: "user", message: { role: "user", content: "[pdo-runtime] You are the pipeline manager." }, timestamp: at() })}\n`,
    });

    let failed = false;
    run.laps.forEach((lap, lapIndex) => {
      const iter = lapIndex + 1;
      for (const nodeId of nodeIds) {
        if (failed) return;
        const attempts = [
          ...(lapIndex === 0 ? run.failedAttempts[nodeId].map((a) => ({ ...a, failed: true })) : []),
          ...lap[nodeId].map((a) => ({ ...a, failed: false })),
        ];
        for (const attempt of attempts) {
          const execution = executionFigures(nodeId, attempt);
          const sessionId = rng.uuid();
          const startedAt = t;
          events.push({
            run_id: runId,
            ts: at(),
            kind: "node_started",
            node_id: nodeId,
            iter,
            payload: {
              node_type: "agent",
              interactive: false,
              orchestrator: false,
              isolated_worktree: false,
              model: attempt.profile.model,
              effort: attempt.profile.effort,
              harness: attempt.profile.harness,
              session_id: sessionId,
              skills: [],
              missing_skills: [],
              skipped_skills: [],
            },
          });
          t += execution.durationMs;
          const steering = !attempt.failed && rng.next() < STEERING_SHARE
            ? rng.pick(STEERING_MESSAGES)
            : null;
          files.push(
            transcriptFile({
              profile: attempt.profile,
              sessionId,
              worktree,
              execution,
              startedAt,
              steering,
              prompt: `${task}.`,
              nextMessageId,
              rng,
            }),
          );
          if (attempt.failed) {
            events.push({
              run_id: runId,
              ts: at(),
              kind: "node_failed",
              node_id: nodeId,
              iter,
              payload: { reason: attempt.profile.harness === "pi" ? "agent exited: provider returned 429" : "agent exited before pdo complete" },
            });
            t += 6_000;
            if (run.status === "failed") {
              failed = true;
              return;
            }
          } else {
            events.push({ run_id: runId, ts: at(), kind: "node_completed", node_id: nodeId, iter, payload: null });
            t += 5_000;
          }
        }
      }
    });
    events.push({
      run_id: runId,
      ts: at(),
      kind: run.status === "failed" ? "run_failed" : "run_completed",
      node_id: null,
      iter: null,
      payload: run.status === "failed" ? { reason: "implementer failed" } : null,
    });
  });

  return { events, files, fires: planFires({ now, days, firedRuns, rng }), prices: MANUAL_PRICES };
}

/** Cost, duration and context of one attempt, from its node's baseline. */
export function executionFigures(nodeId, attempt) {
  const base = NODE_BASELINES[nodeId];
  const { profile, z } = attempt;
  const noise = Math.exp(COST_SPREAD * z);
  const share = attempt.failed ? 0.45 : 1;
  return {
    usd: base.usd * profile.costMult * noise * share,
    durationMs: Math.round(base.minutes * profile.speed * Math.exp(0.35 * z) * share * 60_000),
    context: Math.round(base.context * profile.ctxMult * Math.exp(0.2 * z)),
  };
}

/** About a hundred fires of the demo trigger over the period: mostly skipped by
 *  the guard (prod healthy), a few fired (the incident runs). */
function planFires({ now, days, firedRuns, rng }) {
  const fires = firedRuns.map(({ runId, ts }) => ({
    ts,
    outcome: "fired",
    reason: null,
    run_id: runId,
    guard_stdout: INCIDENT_REPORT,
    guard_stderr: "",
    guard_exit_code: 0,
    source: "schedule",
  }));
  const end = now.getTime() - 5 * 60 * 1000;
  const skipped = 100 - fires.length;
  for (let k = 0; k < skipped; k++) {
    const ts = end - Math.floor(rng.next() * days * 86_400_000);
    fires.push({
      ts: new Date(ts).toISOString(),
      outcome: "guard-exit-nonzero",
      reason: "guard exited non-zero",
      run_id: null,
      guard_stdout: `OK — checkout p95 ${rng.int(180, 420)} ms, 5xx 0.${rng.int(0, 3)} %`,
      guard_stderr: "",
      guard_exit_code: 1,
      source: "schedule",
    });
  }
  return fires.sort((a, b) => a.ts.localeCompare(b.ts));
}

/** A run id in the daemon's shape: `YYYYMMDD-HHMMSS-<7 hex>`. */
function runIdAt(date, rng) {
  const p = (n) => String(n).padStart(2, "0");
  const stamp = `${date.getUTCFullYear()}${p(date.getUTCMonth() + 1)}${p(date.getUTCDate())}-${p(date.getUTCHours())}${p(date.getUTCMinutes())}${p(date.getUTCSeconds())}`;
  return `${stamp}-${rng.hex(7)}`;
}

/** Claude Code's project folder for a cwd (`stale_detector::encode_working_dir`). */
export function encodeClaudeDir(dir) {
  return dir.replace(/[^A-Za-z0-9]/g, "-");
}

/** pi's session folder for a cwd (`pi_session::session_dir_name`). */
export function encodePiDir(dir) {
  return `--${dir.replace(/^[/\\]/, "").replace(/[/\\:]/g, "-")}--`;
}

/** Split an execution's context growth into `n` turns. */
function turns(n, context) {
  const out = [];
  let previous = 0;
  for (let i = 1; i <= n; i++) {
    const ctx = Math.round(12_000 + ((context - 12_000) * i) / n);
    out.push({ ctx, delta: ctx - previous });
    previous = ctx;
  }
  return out;
}

function transcriptFile({ profile, sessionId, worktree, execution, startedAt, steering, prompt, nextMessageId, rng }) {
  const n = rng.int(8, 16);
  const stamp = (i) => new Date(startedAt + Math.round((execution.durationMs * i) / (n + 1))).toISOString();
  if (profile.harness === "claude") {
    return { path: path.join(".claude", "projects", encodeClaudeDir(worktree), `${sessionId}.jsonl`), content: claudeTranscript({ profile, execution, n, stamp, steering, prompt, sessionId, nextMessageId }) };
  }
  if (profile.harness === "copilot") {
    return { path: path.join(".copilot", "session-state", sessionId, "events.jsonl"), content: copilotJournal({ profile, execution, n, stamp, prompt, steering }) };
  }
  const fileStamp = new Date(startedAt).toISOString().replace(/[:.]/g, "-");
  return {
    path: path.join(".pi", "agent", "sessions", encodePiDir(worktree), `${fileStamp}_${sessionId}.jsonl`),
    content: piSession({ profile, execution, n, stamp, prompt, steering, nextMessageId }),
  };
}

/**
 * A claude transcript whose derived cost (`run_cost::line_cost` × the manual
 * price) is exactly `execution.usd`: the context grows turn by turn (cache
 * reads + creations), and the output tokens carry the rest of the budget.
 */
export function claudeTranscript({ profile, execution, n, stamp, steering, prompt, sessionId, nextMessageId }) {
  const price = MANUAL_PRICES[profile.model];
  const perTok = (p) => p / 1_000_000;
  let steps = turns(n, execution.context);
  const cacheCost = (list) => list.reduce((sum, s) => sum + (s.ctx - s.delta) * perTok(price.input) * 0.1 + s.delta * perTok(price.input) * 1.25 + 40 * perTok(price.input), 0);
  const ceiling = execution.usd * 0.75;
  let base = cacheCost(steps);
  if (base > ceiling) {
    const scale = ceiling / base;
    steps = steps.map((s) => ({ ctx: Math.round(s.ctx * scale), delta: Math.round(s.delta * scale) }));
    base = cacheCost(steps);
  }
  const output = Math.max(1, Math.round((execution.usd - base) / (n * perTok(price.output))));
  const lines = [
    { type: "user", sessionId, timestamp: stamp(0), message: { role: "user", content: prompt } },
  ];
  steps.forEach((s, i) => {
    lines.push({
      type: "assistant",
      sessionId,
      requestId: `req_${nextMessageId()}`,
      timestamp: stamp(i + 1),
      message: {
        id: nextMessageId(),
        role: "assistant",
        model: profile.model,
        usage: {
          input_tokens: 40,
          output_tokens: output,
          cache_read_input_tokens: s.ctx - s.delta,
          cache_creation_input_tokens: s.delta,
        },
      },
    });
    if (steering && i === Math.floor(n / 2)) {
      lines.push({ type: "user", sessionId, timestamp: stamp(i + 1), message: { role: "user", content: steering } });
    }
  });
  return `${lines.map((l) => JSON.stringify(l)).join("\n")}\n`;
}

/** A copilot journal: the model and effort at session start, cumulative usage
 *  checkpoints ending on `execution.usd` in nano-AIU. */
export function copilotJournal({ profile, execution, n, stamp, prompt, steering }) {
  const lines = [
    { type: "session.start", timestamp: stamp(0), data: { selectedModel: profile.model, reasoningEffort: profile.effort } },
    { type: "user.message", timestamp: stamp(0), data: { content: prompt } },
  ];
  const totalNano = Math.round(execution.usd * NANO_AIU_PER_USD);
  let input = 0;
  let output = 0;
  turns(n, execution.context).forEach((s, i) => {
    input += s.ctx;
    output += 900;
    lines.push({
      type: "session.usage_checkpoint",
      timestamp: stamp(i + 1),
      data: {
        totalNanoAiu: Math.round((totalNano * (i + 1)) / n),
        usage: { inputTokens: input, outputTokens: output },
        modelCacheState: [{ modelId: profile.model }],
      },
    });
    if (steering && i === Math.floor(n / 2)) {
      lines.push({ type: "user.message", timestamp: stamp(i + 1), data: { content: steering } });
    }
  });
  return `${lines.map((l) => JSON.stringify(l)).join("\n")}\n`;
}

/** A pi session: the thinking level, then assistant messages via openrouter,
 *  each reporting its own cost in USD (summing to `execution.usd`). */
export function piSession({ profile, execution, n, stamp, prompt, steering, nextMessageId }) {
  const lines = [
    { type: "thinking_level_change", id: nextMessageId(), timestamp: stamp(0), thinkingLevel: profile.effort },
    { type: "message", id: nextMessageId(), timestamp: stamp(0), message: { role: "user", content: [{ type: "text", text: prompt }] } },
  ];
  turns(n, execution.context).forEach((s, i) => {
    lines.push({
      type: "message",
      id: nextMessageId(),
      timestamp: stamp(i + 1),
      message: {
        role: "assistant",
        model: profile.model,
        provider: "openrouter",
        usage: { input: s.delta, output: 700, cacheRead: s.ctx - s.delta, totalTokens: s.ctx + 700, cost: { total: execution.usd / n } },
        stopReason: "stop",
      },
    });
    if (steering && i === Math.floor(n / 2)) {
      lines.push({ type: "message", id: nextMessageId(), timestamp: stamp(i + 1), message: { role: "user", content: [{ type: "text", text: steering }] } });
    }
  });
  return `${lines.map((l) => JSON.stringify(l)).join("\n")}\n`;
}
