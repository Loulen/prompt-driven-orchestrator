#!/usr/bin/env node
// `make run-costs RUN=<run-id>`: the cost and duration per ticket of a Run and
// its child Runs, as a markdown table. Read only: `GET /runs/<id>` and
// `GET /runs/<id>/children` on the daemon of `URL=`, else `PDO_DAEMON_URL`,
// with `PDO_TOKEN` as the bearer when set. `OUT=<file>` also writes the table.

import fs from "node:fs";
import path from "node:path";
import { costRows, renderCosts } from "./lib/costs.mjs";

const runId = process.env.RUN;
const base = (process.env.URL || process.env.PDO_DAEMON_URL || "").replace(/\/+$/, "");
if (!runId || !base) {
  console.error("run-costs: RUN=<run-id> is required, and URL=<daemon url> when PDO_DAEMON_URL is not set");
  process.exit(2);
}
const headers = process.env.PDO_TOKEN ? { Authorization: `Bearer ${process.env.PDO_TOKEN}` } : {};

async function get(route) {
  const res = await fetch(`${base}${route}`, { headers });
  if (!res.ok) throw new Error(`GET ${route}: HTTP ${res.status}`);
  return res.json();
}

/** `{ run, children }`, the children read from their summaries' ids, recursively. */
async function tree(id) {
  const [run, kids] = await Promise.all([get(`/runs/${id}`), get(`/runs/${id}/children`)]);
  const ids = (kids.nodes ?? []).flatMap((n) => n.children.map((c) => c.run_id));
  return { run, children: await Promise.all(ids.map(tree)) };
}

try {
  const markdown = renderCosts(costRows(await tree(runId), Date.now()));
  console.log(markdown);
  if (process.env.OUT) {
    fs.mkdirSync(path.dirname(path.resolve(process.env.OUT)), { recursive: true });
    fs.writeFileSync(process.env.OUT, `${markdown}\n`);
  }
} catch (err) {
  console.error(`run-costs: ${err.message}`);
  process.exit(1);
}
