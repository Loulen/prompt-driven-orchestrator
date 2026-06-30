import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { expectNonZeroBBox } from "./assertions";

// Layer 3b — Initial prompt section (#26).
// Verifies: selecting a running node shows the initial prompt containing
// ## Inputs text in the right panel.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-initial-prompt-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);
const PROMPTS_DIR = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.prompts`);

const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - { name: user_prompt, side: bottom }
    view: { x: 100, y: 0 }
  - id: worker
    name: worker
    type: doc-only
    prompt_file: ${PIPELINE_NAME}.prompts/worker.md
    inputs:
      - { name: task, side: top }
    outputs:
      - { name: result, side: bottom }
    view: { x: 100, y: 150 }
  - id: end
    name: End
    type: end
    inputs:
      - { name: result, side: top }
    view: { x: 100, y: 300 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: result }
    target: { node: end, port: result }
`;

const ROLE_PROMPT = "You are a worker. Do the task.\n";

test.beforeAll(async () => {
  process.env.PDO_TMUX_CMD_OVERRIDE = "exec sleep 300";
  await fs.mkdir(PROMPTS_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
  await fs.writeFile(path.join(PROMPTS_DIR, "worker.md"), ROLE_PROMPT);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await fs.rm(PROMPTS_DIR, { recursive: true, force: true });
  delete process.env.PDO_TMUX_CMD_OVERRIDE;
});

test("selecting a running node shows initial prompt with ## Inputs", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  // Create a run via the API
  const resp = await page.request.post(`${baseURL}/runs`, {
    multipart: {
      pipeline: PIPELINE_NAME,
      input: "e2e initial prompt test",
    },
  });
  expect(resp.status()).toBe(201);
  const { run_id } = await resp.json();

  // Wait for the run to appear in the list and click it
  await page.getByText(run_id.slice(0, 8)).first().click({ timeout: 5_000 });

  const reactFlow = page.locator(".react-flow");
  await expect(reactFlow).toBeVisible({ timeout: 5_000 });
  await expectNonZeroBBox(reactFlow);

  // The worker node only spawns after the start node auto-completes; wait for
  // it to enter a running state so its prompt becomes fetchable.
  await page.waitForTimeout(1_500);
  const workerNode = page.getByText("worker", { exact: true }).first();
  await expect(workerNode).toBeVisible({ timeout: 5_000 });
  await expectNonZeroBBox(workerNode);
  await workerNode.click();

  // Selecting a run auto-opens the run for edit, so the right pane is the
  // tabbed InspectorTabs. NodeDetailPanel (which owns the prompt block) lives
  // in the Run pane — switch to it explicitly.
  const runTab = page.getByTestId("inspector-tab-run");
  if (await runTab.isVisible().catch(() => false)) {
    await runTab.click();
  }

  // #270 regression: entering a live run auto-selects the live node, but its
  // terminal must NOT be expanded by default — the details pane shows first.
  await expect(page.getByTestId("details-pane")).toBeVisible({ timeout: 5_000 });
  await expect(page.getByTestId("terminal-fullsize")).toHaveCount(0);

  // Post-#270 the auto-selected live node opens with the terminal NOT expanded,
  // so the details pane (Inputs/Outputs/Initial Prompt) is visible by default.
  // Only toggle the terminal's expand button when the pane isn't already shown
  // (keeps the spec green in both pre- and post-fix worlds).
  const detailsPane = page.getByTestId("details-pane");
  const expandToggle = page.getByTestId("term-expand");
  await expect(expandToggle).toBeVisible({ timeout: 5_000 });
  if (!(await detailsPane.isVisible())) {
    await expandToggle.click();
  }
  await expect(detailsPane).toBeVisible({ timeout: 5_000 });

  // The Initial Prompt section is collapsed by default (post-refonte
  // NodeDetailPanel.PromptSection) — expand it to reveal the prompt block.
  const promptToggle = page.getByTestId("prompt-toggle");
  await expect(promptToggle).toBeVisible({ timeout: 5_000 });
  await promptToggle.click();

  // The prompt block should contain ## Inputs text
  const promptBlock = page.locator(".prompt-block");
  await expect(promptBlock).toBeVisible({ timeout: 3_000 });

  await expect(async () => {
    const text = await promptBlock.textContent();
    expect(text).toContain("## Inputs");
  }).toPass({ timeout: 5_000 });

  // Also verify it contains ## Outputs
  const text = await promptBlock.textContent();
  expect(text).toContain("## Outputs");

  // Cleanup: kill the tmux session
  const sessionName = `pdo-${run_id}-worker-iter-1`;
  const { execSync } = await import("node:child_process");
  try {
    execSync(`tmux kill-session -t ${sessionName}`, { stdio: "ignore" });
  } catch {
    // session may already be dead
  }
});
