/**
 * The pure half of the e2e run ledger (CORE-341, `run-ledger.ts`): who owns a
 * Run, what counts as a leftover, how a reap answer reads. No I/O here, so the
 * rules are unit-tested (`run-ledger-core.test.ts`) without a daemon or git.
 */
import path from "node:path";

/** One `git worktree list --porcelain` entry. */
export interface WorktreeEntry {
  path: string;
  prunable: boolean;
}

/** What git shows of the runs, read in the repo the suite runs from. */
export interface GitSnapshot {
  /** `pdo/run-*` and `pdo/sub-*` branches. */
  branches: Set<string>;
  /** Every worktree registration, by path. */
  worktrees: Map<string, WorktreeEntry>;
}

/** One thing a Run of the suite left behind. */
export interface Residue {
  runId: string;
  kind: string;
  what: string;
}

/**
 * The Runs this suite owns: those the e2e daemon lists now and did not list at
 * setup, plus those the specs reaped themselves (`cleanupRuns` writes them to
 * the ledger file). A Run listed at setup is never owned.
 */
export function ownedRunIds(
  before: ReadonlySet<string>,
  nowIds: Iterable<string>,
  ledgerIds: Iterable<string>,
): Set<string> {
  const owned = new Set<string>();
  for (const id of [...nowIds, ...ledgerIds]) {
    if (id && !before.has(id)) owned.add(id);
  }
  return owned;
}

/**
 * Read the ledger `cleanupRuns` writes: one `<run id>\t<pipeline>` line per
 * Run a spec reaped itself (the pipeline may be empty).
 */
export function parseLedger(text: string): Map<string, string | undefined> {
  const entries = new Map<string, string | undefined>();
  for (const line of text.split("\n")) {
    const [id, pipeline] = line.split("\t");
    if (id.trim()) entries.set(id.trim(), pipeline?.trim() || undefined);
  }
  return entries;
}

/** The run id a `pdo/run-<id>` / `pdo/sub-<id>…` branch belongs to, among `owned`. */
function branchOwner(branch: string, owned: ReadonlySet<string>): string | undefined {
  for (const id of owned) {
    if (branch === `pdo/run-${id}`) return id;
    // A sub-worktree branch is `pdo/sub-<run id>-<node>…` (teardown.rs).
    if (branch.startsWith(`pdo/sub-${id}-`) || branch === `pdo/sub-${id}`) return id;
  }
  return undefined;
}

/**
 * The git leftovers of the owned Runs: every branch and every worktree
 * registration (live or prunable) that did not exist at setup and belongs to
 * an owned Run. A worktree belongs to a Run when it sits under
 * `<runsDir>/<run id>/`, `runsDir` being the e2e daemon's run store.
 */
export function gitResidue(
  before: GitSnapshot,
  after: GitSnapshot,
  owned: ReadonlySet<string>,
  runsDir: string,
): Residue[] {
  const residue: Residue[] = [];
  for (const branch of [...after.branches].sort()) {
    if (before.branches.has(branch)) continue;
    const runId = branchOwner(branch, owned);
    if (runId) residue.push({ runId, kind: "branch", what: branch });
  }
  const prefix = runsDir.endsWith(path.sep) ? runsDir : runsDir + path.sep;
  for (const entry of [...after.worktrees.values()].sort((a, b) => a.path.localeCompare(b.path))) {
    if (before.worktrees.has(entry.path) || !entry.path.startsWith(prefix)) continue;
    const runId = entry.path.slice(prefix.length).split(path.sep)[0];
    if (!owned.has(runId)) continue;
    residue.push({
      runId,
      kind: entry.prunable ? "prunable worktree" : "worktree",
      what: entry.path,
    });
  }
  return residue;
}

/**
 * Read the answer of `cleanup_run`: `null` when the Run is archived (now or
 * already), else why the reap failed.
 */
export function classifyCleanup(status: number, body: string): string | null {
  if (status === 200) return null;
  if (status === 409 && body.includes("already archived")) return null;
  return `cleanup_run answered ${status}: ${body.trim() || "(empty body)"}`;
}

/** Read the answer of `DELETE /runs/<id>`: `null` when the Run is gone. */
export function classifyForget(status: number, body: string): string | null {
  if ((status >= 200 && status < 300) || status === 404) return null;
  return `DELETE answered ${status}: ${body.trim() || "(empty body)"}`;
}

function isWithin(child: string, parent: string): boolean {
  const rel = path.relative(parent, child);
  return rel === "" || (!rel.startsWith("..") && !path.isAbsolute(rel));
}

/**
 * Whether the net may reap a Run, judged on its `target_repo`: the suite only
 * ever names the repo it runs from or a temp repo. Anything else is a Run this
 * suite did not create, so it is reported instead of archived. The daemon keeps
 * `target_repo` as sent, so `canonical` (a realpath in the caller) resolves it
 * the way `e2eRepo` and `tmpRoot` were (`/var` vs `/private/var` on macOS).
 */
export function isReapable(
  targetRepo: string | null | undefined,
  e2eRepo: string,
  tmpRoot: string,
  canonical: (p: string) => string = path.resolve,
): boolean {
  if (!targetRepo) return true;
  const target = canonical(path.resolve(targetRepo));
  return target === path.resolve(e2eRepo) || (isWithin(target, tmpRoot) && target !== path.resolve(tmpRoot));
}

/** `git branch --list … --format=%(refname:short)` -> branch names. */
export function parseBranchList(stdout: string): string[] {
  return stdout
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean);
}

/** `git worktree list --porcelain` -> entries, `prunable` flagged. */
export function parseWorktreePorcelain(stdout: string): WorktreeEntry[] {
  const entries: WorktreeEntry[] = [];
  let current: WorktreeEntry | undefined;
  for (const line of stdout.split("\n")) {
    if (line.startsWith("worktree ")) {
      current = { path: line.slice("worktree ".length), prunable: false };
      entries.push(current);
    } else if (current && (line === "prunable" || line.startsWith("prunable "))) {
      current.prunable = true;
    }
  }
  return entries;
}

/** The failure message: each leftover under its Run id and pipeline. */
export function formatResidue(
  residue: Residue[],
  pipelines: ReadonlyMap<string, string>,
): string {
  const byRun = new Map<string, Residue[]>();
  for (const r of residue) byRun.set(r.runId, [...(byRun.get(r.runId) ?? []), r]);
  const lines: string[] = [];
  for (const [runId, items] of byRun) {
    lines.push(`  run ${runId} (${pipelines.get(runId) ?? "pipeline unknown"})`);
    for (const r of items) lines.push(`    - ${r.kind}: ${r.what}`);
  }
  return lines.join("\n");
}
