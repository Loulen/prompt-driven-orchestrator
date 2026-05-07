import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { expectNonZeroBBox } from "./assertions";

// Layer 4 — Cold-start integration spec (#37).
// Drives the full flow from empty state: goto("/") → connected → click run →
// bbox check on .react-flow → click node → open output modal → close → toggle
// edit → stop editing. Asserts no console errors throughout.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-cold-start-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".maestro", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);
const PROMPTS_DIR = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.prompts`);

const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: worker
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
edges: []
`;

const ROLE_PROMPT = "You are a worker. Do the task.\n";

let runId: string;

test.beforeAll(async () => {
  process.env.MAESTRO_TMUX_CMD_OVERRIDE =
    "exec sh -c \"printf '\\033[32mhello ansi\\033[0m\\n'; sleep 300\"";
  await fs.mkdir(PROMPTS_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
  await fs.writeFile(path.join(PROMPTS_DIR, "worker.md"), ROLE_PROMPT);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await fs.rm(PROMPTS_DIR, { recursive: true, force: true });
  delete process.env.MAESTRO_TMUX_CMD_OVERRIDE;
  if (runId) {
    const { execSync } = await import("node:child_process");
    try {
      execSync(`tmux kill-session -t maestro-${runId}-worker-iter-1`, {
        stdio: "ignore",
      });
    } catch {
      // session may already be dead
    }
  }
});

test("cold-start full flow: run → node → modal → edit toggle, no console errors", async ({
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
    data: {
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
    ".maestro",
    "runs",
    runId,
    "worktree",
    ".maestro",
    "artifacts",
  );
  const workerArtifactDir = path.join(artifactsDir, "worker", "iter-1");
  await fs.mkdir(workerArtifactDir, { recursive: true });
  await fs.writeFile(
    path.join(workerArtifactDir, "result.md"),
    "---\nverdict: PASS\n---\n\n## Result\n\nAll done.",
  );

  // 3. Click the run from the list
  await page.getByText(runId.slice(0, 8)).first().click({ timeout: 5_000 });

  // 4. Assert .react-flow has non-zero bounding box (canvas-height-0 guard)
  const reactFlow = page.locator(".react-flow");
  await expect(reactFlow).toBeVisible({ timeout: 5_000 });
  await expectNonZeroBBox(reactFlow);

  // 5. Click the worker node
  await page.waitForTimeout(500);
  const workerNode = page.getByText("worker", { exact: true }).first();
  await expect(workerNode).toBeVisible({ timeout: 3_000 });
  await workerNode.click();

  // 6. Wait for Outputs section and open the output modal
  await expect(page.getByText("Outputs")).toBeVisible({ timeout: 5_000 });
  const openLink = page.locator(".open-link").first();
  await expect(openLink).toBeVisible({ timeout: 5_000 });
  await openLink.click();

  // 7. Modal should appear with content
  const modal = page.locator(".artifact-markdown");
  await expect(modal).toBeVisible({ timeout: 3_000 });
  await expect(modal).toContainText("Result");

  // 8. Close modal via Escape
  await page.keyboard.press("Escape");
  await expect(modal).not.toBeVisible({ timeout: 2_000 });

  // 9. Click "Edit this run"
  const editButton = page.getByRole("button", { name: "Edit this run" });
  await expect(editButton).toBeVisible({ timeout: 3_000 });
  await editButton.click();

  // 10. Should see the run-scoped edit view
  await expect(page.getByText("template unchanged")).toBeVisible({
    timeout: 3_000,
  });

  // 11. Click "Stop editing"
  const stopButton = page.getByRole("button", { name: "Stop editing" });
  await expect(stopButton).toBeVisible();
  await stopButton.click();

  // 12. Should be back in run view
  await expect(page.getByRole("button", { name: "Edit this run" })).toBeVisible({
    timeout: 3_000,
  });

  // 13. Assert no console errors during the entire flow
  expect(consoleErrors).toEqual([]);
});
