// The full gate's steps come from the CI the repository declares
// (.github/workflows/ci.yml), read rather than copied: a step added to the CI
// is in `make gate` the next time it runs. Pure.

import { VITEST, nodeTests } from "./commands.mjs";

const JOB = /^ {2}([\w-]+):\s*$/;
// `- run: x`, or `run: x` under a `- name:` (any key of a step can come first).
const RUN = /^\s*(?:- )?run:\s*(.*)$/;
// What changes how or whether a step runs: the gate cannot replay it with a plain `bash -c`.
const UNREPLAYABLE = /^\s+(working-directory|env|shell|if|continue-on-error|timeout-minutes):/;
const STEP = /^\s*- /;

/**
 * The `run:` commands of the workflow `text`, `{ job, command }`, in file order,
 * the jobs of `skipJobs` aside. Refuses what a plain `bash -c` from the repository
 * root would not replay: a multi-line `run:`, a step's `working-directory:`, `env:`,
 * `shell:`, `if:`, `continue-on-error:` or `timeout-minutes:`, a job's `defaults:` or `env:`.
 */
export function ciRunSteps(text, { skipJobs = [] } = {}) {
  const steps = [];
  let job = null;
  let inSteps = false;
  let inRun = false;
  let pending = null; // a key seen before `run:` in the current step that the gate cannot replay
  for (const line of text.split("\n")) {
    const header = JOB.exec(line);
    if (header) {
      job = header[1];
      inSteps = false;
      inRun = false;
      pending = null;
      continue;
    }
    if (job === null || skipJobs.includes(job)) continue;
    const jobKey = /^ {4}(defaults|env):\s*$/.exec(line);
    if (jobKey) throw new Error(`job ${job}: a job-level \`${jobKey[1]}:\` is not replayed by the gate`);
    if (/^ {4}steps:\s*$/.test(line)) {
      inSteps = true;
      continue;
    }
    if (!inSteps) continue;
    const run = RUN.exec(line);
    if (run) {
      const command = run[1].trim();
      if (/^[|>]/.test(command)) throw new Error(`job ${job}: a multi-line \`run:\` is not replayed by the gate`);
      if (pending) {
        throw new Error(`job ${job}: the step \`${command}\` sets ${pending.trim().replace(/^- /, "").split(":")[0]}, which the gate does not replay`);
      }
      steps.push({ job, command });
      inRun = true;
    } else if (STEP.test(line)) {
      // A new step: its keys before `run:` are checked once `run:` is seen.
      inRun = false;
      pending = UNREPLAYABLE.test(line.replace(/^(\s*)- /, "$1  ")) ? line : null;
    } else if (UNREPLAYABLE.test(line)) {
      if (inRun) {
        const key = line.trim().split(":")[0];
        throw new Error(`job ${job}: the step \`${steps.at(-1).command}\` sets ${key}, which the gate does not replay`);
      }
      pending = line;
    }
  }
  return steps;
}

const EXTRA = [
  // Not in the CI, run by `make test`: vitest and the node tests of the scripts.
  { name: "vitest", command: VITEST },
  { name: "scripts tests", command: nodeTests("scripts/*/test/*.test.mjs") },
];

/**
 * The full gate: each CI step once (an install both jobs do runs once), nextest
 * listing every failure instead of stopping at the first, then vitest and the
 * script tests. `[{ name, command, source }]`.
 */
export function gateSteps(ciSteps) {
  const seen = new Set();
  const steps = [];
  for (const { job, command } of ciSteps) {
    const replayed = /^cargo nextest run\b/.test(command) && !/--no-fail-fast/.test(command) ? `${command} --no-fail-fast` : command;
    if (seen.has(replayed)) continue;
    seen.add(replayed);
    steps.push({ name: replayed, command: replayed, source: `CI ${job}` });
  }
  for (const extra of EXTRA) {
    if (!seen.has(extra.command)) steps.push({ ...extra, source: "not in CI" });
  }
  return steps;
}
