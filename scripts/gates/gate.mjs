#!/usr/bin/env node
// `make gate`, the full gate: every step the CI declares (.github/workflows/ci.yml),
// the E2E job aside (`make e2e`, before `integration -> main`), nextest without
// fail-fast, plus vitest and the script tests. Run once per ticket, after the FP.
// A failed step does not stop the others: every failure is listed at the end.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { ciRunSteps, gateSteps } from "./lib/ci.mjs";
import { runSteps, shellExec } from "./lib/run.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const CI = ".github/workflows/ci.yml";

try {
  const steps = gateSteps(ciRunSteps(fs.readFileSync(path.join(root, CI), "utf8"), { skipJobs: ["e2e"] }));
  console.log(`==> Full gate: ${steps.length} steps, from ${CI} (E2E job aside) plus vitest and the script tests`);
  for (const step of steps) console.log(`      - ${step.command}  (${step.source})`);
  process.exit(await runSteps(steps, { exec: shellExec(root) }));
} catch (err) {
  console.error(`full gate: ${err.message}`);
  process.exit(2);
}
