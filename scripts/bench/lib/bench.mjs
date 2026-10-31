// One bench: a throwaway worktree at a ref, the measures one after the other,
// the worktree removed whatever happens. Every effect goes through `ops`, so the
// sequence is tested without git, cargo or make.

import { benchMeasures, makeTargets } from "./plan.mjs";

/**
 * The outcome of a measure from its exit code and whether its output named the
 * injected file: an expected failure is « caught » only when it names it.
 */
export function outcome(measure, code, sawMarker) {
  if (measure.expect === "fail") {
    if (code === 0) return "not caught";
    return measure.marker && !sawMarker ? `failed for another reason (exit ${code})` : "caught";
  }
  return code === 0 ? "ok" : `failed (exit ${code})`;
}

/**
 * Runs the bench of `ref`. `ops`:
 * - `addWorktree(ref)` -> `{ dir, sha }`, `removeWorktree(dir)`;
 * - `readMakefile(dir)`, `sccache()` -> its version or null;
 * - `shell(dir, line)` -> exit code (setup/cleanup, not timed);
 * - `run(dir, measure)` -> `{ code, seconds, output }` (the output is searched for the marker);
 * - `freeGb()`, `load()`, `now()` -> ISO date;
 * - `startCache()` / `stopCache()`: the private sccache of the wrapped builds.
 * `minFreeGb`: below it, the measures left are skipped, never run on a full disk.
 */
export async function runBench(ref, { ops, minFreeGb = 25, log = console.log }) {
  const startedAt = ops.now();
  const loadStart = ops.load();
  const { dir, sha } = await ops.addWorktree(ref);
  const measures = [];
  try {
    const sccache = ops.sccache();
    const plan = benchMeasures({ targets: makeTargets(ops.readMakefile(dir)), sccache: Boolean(sccache) });
    if (sccache) await ops.startCache();
    let stop = null;
    for (const m of plan) {
      const base = { id: m.id, label: m.label };
      if (m.na) {
        measures.push({ ...base, na: m.na });
        log(`  ${m.id}: ${m.na}`);
        continue;
      }
      if (stop) {
        measures.push({ ...base, skipped: stop });
        continue;
      }
      if (m.setup && (await ops.shell(dir, m.setup)) !== 0) throw new Error(`${m.id}: setup failed: ${m.setup.split("\n")[0]}`);
      const free = ops.freeGb();
      if (free < minFreeGb) {
        stop = `${free.toFixed(0)} GB free, ${minFreeGb} needed`;
        measures.push({ ...base, skipped: stop });
        log(`  ${m.id}: skipped (${stop})`);
        continue;
      }
      const load = ops.load();
      log(`  ${m.id}: ${m.command} (load ${load.toFixed(1)}) …`);
      const { code, seconds, output } = await ops.run(dir, m);
      if (m.cleanup) await ops.shell(dir, m.cleanup);
      const result = { ...base, ...(m.showCommand ? { command: m.command } : {}), seconds, load, code, outcome: outcome(m, code, m.marker ? output.includes(m.marker) : false) };
      if (m.wrapped && ops.cacheStats) result.note = ops.cacheStats();
      measures.push(result);
      log(`  ${m.id}: ${result.outcome} in ${Math.round(seconds)} s`);
    }
    return { ref, sha, measures, startedAt, finishedAt: ops.now(), loadStart, loadEnd: ops.load() };
  } finally {
    if (ops.stopCache) await ops.stopCache();
    await ops.removeWorktree(dir);
  }
}
