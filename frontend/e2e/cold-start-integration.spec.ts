import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { expectNonZeroBBox } from "./assertions";
import { openRunNodeDetails, cleanupRuns } from "./helpers";

// Layer 4 — Cold-start integration spec (#37).
// Drives the full flow from empty state: goto("/") → connected → click run →
// bbox check on .react-flow → click node → open output modal → close. Asserts
// no console errors throughout.
//
// The run-scoped "Edit this run" / "Stop editing" toggle this spec used to drive
// was removed in #57 (run-scope editing now happens inline in the run-edit tab,
// with no enter/exit affordance), so those steps are gone — there is no current
// UI to assert against. The cold-start contract that remains is: a run renders
// its canvas + node detail + output modal from a cold load with zero console
// errors.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-cold-start-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);
const PROMPTS_DIR = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.prompts`);

// Post-refonte the parser requires exactly one start node (zero inputs, one
// `user_prompt` output) and one end node (zero outputs, one `result` input).
const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
    view: { x: 200, y: 0 }
  - id: worker
    name: worker
    type: doc-only
    prompt_file: ${PIPELINE_NAME}.prompts/worker.md
    inputs:
      - name: task
    outputs:
      - name: result
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
    view: { x: 200, y: 100 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
    view: { x: 200, y: 250 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: result }
    target: { node: end, port: result }
`;

const ROLE_PROMPT = "You are a worker. Do the task.\n";

let runId: string;

test.beforeAll(async () => {
  await fs.mkdir(PROMPTS_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
  await fs.writeFile(path.join(PROMPTS_DIR, "worker.md"), ROLE_PROMPT);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await fs.rm(PROMPTS_DIR, { recursive: true, force: true });
  await cleanupRuns(runId);
});

test("cold-start full flow: run → node → modal, no console errors", async ({
  page,
  baseURL,
}) => {
  // Collect console errors throughout the test
  const consoleErrors: string[] = [];
  page.on("console", (msg) => {
    if (msg.type() === "error") {
      consoleErrors.push(msg.text());
    }
  });

  // 1. Navigate and wait for daemon connection
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  // 2. Create a run via the API and seed an output artifact
  const resp = await page.request.post(`${baseURL}/runs`, {
    multipart: {
      pipeline: PIPELINE_NAME,
      input: "cold-start integration test",
    },
  });
  expect(resp.status()).toBe(201);
  const json = await resp.json();
  runId = json.run_id;

  // Seed an output artifact so the Outputs section is populated
  const artifactsDir = path.join(
    WORKSPACE_ROOT,
    ".pdo",
    "runs",
    runId,
    "worktree",
    ".pdo",
    "artifacts",
  );
  // Output artifacts live at <artifacts>/<node>/iter-<N>/<port>/output.md.
  const workerArtifactDir = path.join(artifactsDir, "worker", "iter-1", "result");
  await fs.mkdir(workerArtifactDir, { recursive: true });
  await fs.writeFile(
    path.join(workerArtifactDir, "output.md"),
    "---\nverdict: PASS\n---\n\n## Result\n\nAll done.",
  );

  // 3. Click the run from the list
  await page.getByText(runId.slice(0, 20)).first().click({ timeout: 5_000, position: { x: 5, y: 5 } });

  // 4. Assert .react-flow has non-zero bounding box (canvas-height-0 guard)
  const reactFlow = page.locator(".react-flow");
  await expect(reactFlow).toBeVisible({ timeout: 5_000 });
  await expectNonZeroBBox(reactFlow);

  // 5. Select the worker node and reveal the Run inspector details pane.
  await openRunNodeDetails(page, runId, "worker");

  // 6. Open the output modal from the seeded `result` output port card. Target
  // it by port name, not `.first()`: since #370 fixed input resolution, the
  // node's resolved input renders as a clickable `button.port-row` too, so the
  // first button is no longer guaranteed to be the output. A no-files port still
  // renders as a non-interactive div. The port name is the button's leading text.
  const portCard = page
    .getByTestId("inspector-pane-run")
    .locator("button.port-row")
    .filter({ hasText: /^result/ });
  await expect(portCard).toBeVisible({ timeout: 5_000 });
  await portCard.click();

  // 7. Modal should appear with content
  const modal = page.locator(".artifact-markdown");
  await expect(modal).toBeVisible({ timeout: 3_000 });
  await expect(modal).toContainText("Result");

  // 8. Close modal via Escape
  await page.keyboard.press("Escape");
  await expect(modal).not.toBeVisible({ timeout: 2_000 });

  // 9. Assert no console errors during the cold-start flow. Ignore transient
  // resource 404s — cross-spec fixture churn (a sibling spec's afterAll deleting
  // its pipeline file while this page's list still references it) surfaces as a
  // network 404, which is harness noise, not a page error.
  expect(consoleErrors.filter((e) => !/Failed to load resource/.test(e))).toEqual([]);
});
