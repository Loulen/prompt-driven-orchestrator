// The cost and duration per ticket of a story: a Run (its own nodes, the
// orchestration) and its child Runs (one per ticket), read from the daemon's
// Run detail (`cost.usd`, `started_at`, `completed_at`) and its children.

import { formatDuration, mdRow } from "./table.mjs";

const usd = (n) => `$${n.toFixed(2)}`;

/** Seconds between `started_at` and `completed_at`, or `now` for a Run still going; 0 for a Run never started. */
export function runSeconds(run, now) {
  if (!run.started_at) return 0;
  const end = run.completed_at ? Date.parse(run.completed_at) : now;
  return Math.max(0, (end - Date.parse(run.started_at)) / 1000);
}

/**
 * The rows of the cost table: the Run itself, then every descendant (depth
 * first, `depth` 1 for a child). `tree` is `{ run, children: [tree] }`.
 */
export function costRows(tree, now) {
  const rows = [];
  const visit = (node, depth) => {
    const r = node.run;
    rows.push({
      depth,
      runId: r.run_id,
      name: r.name || r.pipeline_name || "",
      status: r.status,
      seconds: runSeconds(r, now),
      usd: r.cost?.usd ?? 0,
      // No cost source: counted as 0, never shown as a real $0.
      uncosted: typeof r.cost?.usd !== "number",
      partial: Boolean(r.cost?.partial) || !r.completed_at || typeof r.cost?.usd !== "number",
    });
    for (const child of node.children) visit(child, depth + 1);
  };
  visit(tree, 0);
  return rows;
}

const median = (xs) => {
  if (!xs.length) return 0;
  const s = [...xs].sort((a, b) => a - b);
  const mid = Math.floor(s.length / 2);
  return s.length % 2 ? s[mid] : (s[mid - 1] + s[mid]) / 2;
};

/** Totals: the whole tree, its tickets (the direct children), mean and median per ticket. */
export function costSummary(rows) {
  const tickets = rows.filter((r) => r.depth === 1);
  const ticketTotal = (r) => rows.filter((x) => x === r || isUnder(rows, r, x)).reduce((a, x) => a + x.usd, 0);
  const perTicket = tickets.map(ticketTotal);
  return {
    total: rows.reduce((a, r) => a + r.usd, 0),
    orchestration: rows[0]?.usd ?? 0,
    tickets: tickets.length,
    meanUsd: perTicket.length ? perTicket.reduce((a, b) => a + b, 0) / perTicket.length : 0,
    medianUsd: median(perTicket),
    meanSeconds: tickets.length ? tickets.reduce((a, r) => a + r.seconds, 0) / tickets.length : 0,
    medianSeconds: median(tickets.map((r) => r.seconds)),
    partial: rows.some((r) => r.partial),
  };
}

/** Whether `x` sits under the ticket `t` in the depth-first `rows`. */
function isUnder(rows, t, x) {
  const i = rows.indexOf(t);
  const j = rows.indexOf(x);
  if (j <= i) return false;
  for (let k = i + 1; k <= j; k++) if (rows[k].depth <= t.depth) return false;
  return true;
}

/** The markdown: one row per Run, then the totals. */
export function renderCosts(rows) {
  const s = costSummary(rows);
  const out = [mdRow(["Run", "Name", "Status", "Duration", "Cost"]), mdRow(["---", "---", "---", "---", "---"])];
  for (const r of rows) {
    const name = r.depth === 0 ? `${r.name} (orchestration, its own nodes)` : `${"  ".repeat(r.depth - 1)}${r.name}`;
    out.push(mdRow([`\`${r.runId}\``, name, r.status, formatDuration(r.seconds), r.uncosted ? "- (no cost)" : `${usd(r.usd)}${r.partial ? " (partial)" : ""}`]));
  }
  out.push("");
  out.push(`Total: ${usd(s.total)}${s.partial ? " (partial: a Run is still going or not fully costed)" : ""}, of which orchestration ${usd(s.orchestration)}.`);
  out.push(
    `Per ticket (${s.tickets} child Runs, their own children included): mean ${usd(s.meanUsd)}, median ${usd(s.medianUsd)}; duration mean ${formatDuration(s.meanSeconds)}, median ${formatDuration(s.medianSeconds)}.`,
  );
  return out.join("\n");
}
