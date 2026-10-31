// Runs the steps of a gate one after the other, never stopping at a failure,
// then lists every failed step with the lines that name what failed.

import { spawn } from "node:child_process";

const ANSI = /\u001b\[[0-9;]*[A-Za-z]/g;
// nextest/vitest `FAIL`, rustc/cargo `error:`/`error[E…]:`, vitest `×`, node --test `not ok`.
const FAILURE = /\bFAIL\b|^error(\[E\d+\])?:|^\s*[×✗]\s|^\s*not ok\b/;
const MAX_LINES = 20;
const TAIL = 5;

/** The lines of `output` that name a failure (at most 20), or its last lines when none does. */
export function failureLines(output) {
  const lines = output.replace(ANSI, "").split("\n").map((l) => l.replace(/\r$/, ""));
  const found = [...new Set(lines.filter((l) => FAILURE.test(l)))];
  if (!found.length) return lines.filter((l) => l.trim()).slice(-TAIL);
  if (found.length <= MAX_LINES) return found;
  return [...found.slice(0, MAX_LINES), `… (+${found.length - MAX_LINES} more)`];
}

const seconds = (ms) => `${(ms / 1000).toFixed(1)} s`;

/** Runs `command` under bash from `cwd`, its output shown as it comes and kept. */
export function shellExec(cwd) {
  return (step) =>
    new Promise((resolve) => {
      const child = spawn("bash", ["-c", step.command], { cwd, stdio: ["ignore", "pipe", "pipe"] });
      let output = "";
      child.stdout.on("data", (chunk) => {
        process.stdout.write(chunk);
        output += chunk;
      });
      child.stderr.on("data", (chunk) => {
        process.stderr.write(chunk);
        output += chunk;
      });
      child.on("error", (err) => resolve({ code: 127, output: `${output}\nerror: ${err.message}` }));
      child.on("close", (code, signal) => resolve({ code: code ?? (signal ? 128 : 1), output }));
    });
}

/**
 * Runs `steps` (`{ name, command, reason? }`) in order with `exec`, whatever fails.
 * Returns the exit code of the gate: 0 when every step passed, 1 otherwise.
 */
export async function runSteps(steps, { exec, log = console.log, now = Date.now }) {
  const failed = [];
  const timings = [];
  const started = now();
  for (const [i, step] of steps.entries()) {
    const label = step.name === step.command ? step.command : `${step.name}: ${step.command}`;
    log(`\n==> [${i + 1}/${steps.length}] ${label}`);
    if (step.reason) log(`    why: ${step.reason}`);
    const t0 = now();
    const { code, output } = await exec(step);
    const took = now() - t0;
    timings.push({ step, took, code });
    if (code === 0) log(`<== ok ${step.name} in ${seconds(took)}`);
    else {
      log(`<== FAILED ${step.name} (exit ${code}) in ${seconds(took)}`);
      failed.push({ step, code, lines: failureLines(output) });
    }
  }
  const total = now() - started;
  log("\n==> Steps");
  for (const { step, took, code } of timings) log(`    ${code === 0 ? "ok    " : "FAILED"} ${seconds(took).padStart(8)}  ${step.name}`);
  if (!failed.length) {
    log(`all ${steps.length} steps passed in ${seconds(total)}`);
    return 0;
  }
  log(`${failed.length} of ${steps.length} steps failed (${seconds(total)}):`);
  for (const { step, code, lines } of failed) {
    log(`\nFAILED ${step.name} (exit ${code}): ${step.command}`);
    for (const line of lines) log(`    ${line}`);
  }
  return 1;
}
