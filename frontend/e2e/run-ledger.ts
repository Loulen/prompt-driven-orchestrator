/**
 * The run ledger (CORE-341): the suite's safety net for the Runs it creates.
 *
 * Every Run a spec creates on the e2e daemon leaves a `pdo/run-<id>` branch and
 * a worktree registration in the **common gitdir** of the repo the suite runs
 * from, which is the maintainer's shared repo when a node runs `make e2e` from a
 * worktree. Not every spec calls `cleanupRuns`, and a UI-created Run has no id
 * in the spec at all. So this `globalSetup` snapshots before the suite and its
 * teardown (the function it returns) reaps after it:
 *
 * 1. **Ownership**: a Run is the suite's when the e2e daemon lists it at
 *    teardown and did not at setup, or when a spec reaped it itself
 *    (`cleanupRuns` appends its id to the ledger file named by
 *    `PDO_E2E_RUN_LEDGER`). A Run listed at setup is never touched: leftovers
 *    of an older suite stay until someone cleans them by hand.
 * 2. **Reap through the daemon only**: `cleanup_run` (it removes the
 *    worktrees and the `pdo/run-<id>` / `pdo/sub-<id>*` branches, and the node
 *    sessions) then `DELETE /runs/<id>`. Git is only **read** here, so a fleet
 *    Run's branch or worktree is never touched. A Run whose `target_repo` is
 *    neither this repo nor a temp repo is reported, never reaped.
 * 3. **Assert**: the teardown throws (the suite fails) when an owned Run is
 *    still listed, a reap call failed, or a branch / worktree registration
 *    (live or prunable) that did not exist at setup remains for an owned Run.
 *    `cleanup_run` ignores a failed `git branch -D`, hence the re-read.
 *
 * Playwright starts the `webServer` before `globalSetup` and stops it after
 * the teardown, so the daemon answers both ends.
 *
 * Residual risk with `reuseExistingServer` (local): a Run created on the
 * **same** e2e daemon during the suite is reaped too, by hand or by a second
 * suite sharing that daemon (two `make e2e` on one port): the first suite to
 * finish reaps the other's Runs and breaks it. Give each concurrent suite its
 * own `E2E_PORT` (nodes already do). The fleet daemon is on another port and
 * is never queried.
 *
 * Debugging: `PDO_E2E_KEEP_RUNS=1` keeps every Run and skips the assertion.
 */
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import { E2E_BASE_URL, E2E_TARGET_REPO } from "./helpers";
import {
  classifyCleanup,
  classifyForget,
  formatResidue,
  gitResidue,
  isReapable,
  ownedRunIds,
  parseBranchList,
  parseLedger,
  parseWorktreePorcelain,
  type GitSnapshot,
  type Residue,
} from "./run-ledger-core";

interface RunRow {
  run_id: string;
  pipeline_name?: string;
  target_repo?: string | null;
  parent_run_id?: string | null;
  status?: string;
}

const REPO = fs.realpathSync(E2E_TARGET_REPO);
/** The e2e daemon's run store: its repo root is the repo the suite runs from. */
const RUNS_DIR = path.join(REPO, ".pdo", "runs");
const TMP_ROOT = fs.realpathSync(os.tmpdir());

async function listRuns(): Promise<RunRow[]> {
  const resp = await fetch(`${E2E_BASE_URL}/runs`);
  if (!resp.ok) throw new Error(`run ledger: GET /runs answered ${resp.status}`);
  return (await resp.json()) as RunRow[];
}

function git(args: string[]): string {
  // Read the repo the suite runs from, whatever git env a PDO node exported.
  const env = { ...process.env };
  for (const k of ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_COMMON_DIR"]) delete env[k];
  return execFileSync("git", args, { cwd: REPO, env, encoding: "utf8" });
}

function gitSnapshot(): GitSnapshot {
  const branches = parseBranchList(
    git(["branch", "--list", "pdo/run-*", "pdo/sub-*", "--format=%(refname:short)"]),
  );
  const worktrees = parseWorktreePorcelain(git(["worktree", "list", "--porcelain"]));
  return {
    branches: new Set(branches),
    worktrees: new Map(worktrees.map((w) => [w.path, w])),
  };
}

function readLedger(file: string): Map<string, string | undefined> {
  try {
    return parseLedger(fs.readFileSync(file, "utf8"));
  } catch {
    return new Map();
  }
}

/** Realpath when the path exists, else as resolved. */
function canonical(p: string): string {
  try {
    return fs.realpathSync(p);
  } catch {
    return path.resolve(p);
  }
}

/** Children before their parent, so a cascade never meets a live child. */
function deepestFirst(runs: RunRow[]): RunRow[] {
  const byId = new Map(runs.map((r) => [r.run_id, r]));
  const depth = (r: RunRow): number => {
    let d = 0;
    for (let p = r.parent_run_id; p && byId.has(p) && d < runs.length; p = byId.get(p)?.parent_run_id) d++;
    return d;
  };
  return [...runs].sort((a, b) => depth(b) - depth(a));
}

async function reap(run: RunRow): Promise<string[]> {
  const failures: string[] = [];
  const cleanup = await fetch(`${E2E_BASE_URL}/runs/${run.run_id}/commands`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ kind: "cleanup_run" }),
  });
  const cleanupFailure = classifyCleanup(cleanup.status, await cleanup.text());
  if (cleanupFailure) return [cleanupFailure];
  const forget = await fetch(`${E2E_BASE_URL}/runs/${run.run_id}`, { method: "DELETE" });
  const forgetFailure = classifyForget(forget.status, await forget.text());
  if (forgetFailure) failures.push(forgetFailure);
  return failures;
}

export default async function globalSetup(): Promise<() => Promise<void>> {
  const before = new Set((await listRuns()).map((r) => r.run_id));
  const gitBefore = gitSnapshot();
  const ledgerDir = fs.mkdtempSync(path.join(os.tmpdir(), "pdo-e2e-run-ledger-"));
  const ledgerFile = path.join(ledgerDir, "reaped");
  // Workers start after globalSetup, so they inherit it (helpers.ts cleanupRuns).
  process.env.PDO_E2E_RUN_LEDGER = ledgerFile;
  if (before.size > 0) {
    console.log(
      `[run ledger] the e2e daemon already holds ${before.size} run(s) from before this suite; ` +
        "they are never reaped (clean them by hand if an older suite leaked them).",
    );
  }

  return async function teardown(): Promise<void> {
    const ledger = readLedger(ledgerFile);
    fs.rmSync(ledgerDir, { recursive: true, force: true });
    const now = await listRuns();
    const owned = ownedRunIds(before, now.map((r) => r.run_id), ledger.keys());
    const listedOwned = now.filter((r) => owned.has(r.run_id));

    if (process.env.PDO_E2E_KEEP_RUNS === "1") {
      console.log(
        `[run ledger] PDO_E2E_KEEP_RUNS=1: kept ${listedOwned.length} run(s), no assertion: ` +
          (listedOwned.map((r) => r.run_id).join(", ") || "none"),
      );
      return;
    }

    const pipelines = new Map<string, string>();
    for (const [id, pipeline] of ledger) if (pipeline) pipelines.set(id, pipeline);
    for (const r of now) if (r.pipeline_name) pipelines.set(r.run_id, r.pipeline_name);
    const residue: Residue[] = [];
    const reaped: RunRow[] = [];
    for (const run of deepestFirst(listedOwned)) {
      if (!isReapable(run.target_repo, REPO, TMP_ROOT, canonical)) {
        residue.push({
          runId: run.run_id,
          kind: "not reaped",
          what: `target_repo ${run.target_repo} is neither ${REPO} nor a temp repo`,
        });
        continue;
      }
      try {
        const failures = await reap(run);
        for (const what of failures) residue.push({ runId: run.run_id, kind: "reap failed", what });
        if (failures.length === 0) reaped.push(run);
      } catch (e) {
        residue.push({ runId: run.run_id, kind: "reap failed", what: String(e) });
      }
    }

    const after = await listRuns();
    for (const run of after) {
      if (owned.has(run.run_id) && !residue.some((r) => r.runId === run.run_id)) {
        residue.push({ runId: run.run_id, kind: "run still listed", what: `status ${run.status ?? "?"}` });
      }
    }
    residue.push(...gitResidue(gitBefore, gitSnapshot(), owned, RUNS_DIR));

    if (reaped.length > 0) {
      console.warn(
        `[run ledger] ${reaped.length} run(s) reaped by the net (a spec created them without cleanupRuns): ` +
          reaped
            .map((r) => `${r.run_id} (${pipelines.get(r.run_id)})`)
            .join(", "),
      );
    }
    if (residue.length > 0) {
      throw new Error(
        "[run ledger] the e2e suite left residue in the repo it ran from " +
          `(${REPO}); fix the spec or the daemon's teardown:\n` +
          formatResidue(residue, pipelines),
      );
    }
  };
}
