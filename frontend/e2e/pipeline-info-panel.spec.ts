import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Layer 3b — Pipeline info panel (refs #56, #69, ADR 0004).
// Verifies:
// 1. Clicking the toolbar `i` icon opens the PipelineInfoPanel.
// 2. The panel shows pipeline metadata (name).
// 3. During a Run, a terminal element is present for the manager session.
// 4. Clicking a node in the canvas auto-closes the info panel (#69).
// 5. YAML tab shows serialized pipeline and updates on canvas mutation (#69).

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-info-panel-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".maestro", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);

const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
variables:
  max_iter:
    type: int
    default: 3
nodes:
  - id: worker
    type: doc-only
    inputs:
      - name: in
    outputs:
      - name: out
    view: { x: 100, y: 100 }
edges: []
`;

test.beforeAll(async () => {
  process.env.MAESTRO_TMUX_CMD_OVERRIDE = 'exec sh -c "cat"';
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  delete process.env.MAESTRO_TMUX_CMD_OVERRIDE;
});

test("clicking toolbar info opens pipeline info panel with metadata", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  // Create a run to get into run mode
  const resp = await page.request.post(`${baseURL}/runs`, {
    data: {
      pipeline: PIPELINE_NAME,
      input: "e2e info panel test",
    },
  });
  expect(resp.status()).toBe(201);
  const { run_id } = await resp.json();

  // Select the run in the left panel
  await page.getByText(run_id.slice(0, 8)).first().click({ timeout: 5_000 });
  await page.waitForTimeout(500);

  // Click the toolbar info button
  const infoBtn = page.getByTestId("toolbar-info");
  await expect(infoBtn).toBeVisible({ timeout: 3_000 });
  await infoBtn.click();

  // Assert the pipeline info panel renders
  const infoPanel = page.getByTestId("pipeline-info-panel");
  await expect(infoPanel).toBeVisible({ timeout: 3_000 });

  // Assert pipeline name is shown
  await expect(page.getByTestId("info-panel-name")).toContainText(
    PIPELINE_NAME,
  );

  // Assert variables section is present
  await expect(page.getByTestId("info-panel-variables")).toBeVisible();

  // Assert the manager terminal is rendered (run is active)
  const terminal = infoPanel.getByTestId("tmux-terminal");
  await expect(terminal).toBeVisible({ timeout: 5_000 });

  // Close the panel
  await page.getByTestId("info-panel-close").click();
  await expect(infoPanel).not.toBeVisible();

  // Cleanup tmux sessions
  const { execSync } = await import("node:child_process");
  try {
    execSync(`tmux kill-session -t maestro-${run_id}-worker-iter-1`, {
      stdio: "ignore",
    });
  } catch {
    // ok
  }
  try {
    execSync(`tmux kill-session -t maestro-mgr-${run_id}`, {
      stdio: "ignore",
    });
  } catch {
    // ok
  }
});

test("clicking a node closes the pipeline info panel (#69)", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  // Enter edit mode and open the pipeline
  await page.locator('[title="Toggle edit mode"]').click();
  await page.getByRole("button", { name: new RegExp(PIPELINE_NAME) }).click();

  // Open the pipeline info panel via toolbar
  const infoBtn = page.getByTestId("toolbar-info");
  await expect(infoBtn).toBeVisible({ timeout: 3_000 });
  await infoBtn.click();

  const infoPanel = page.getByTestId("pipeline-info-panel");
  await expect(infoPanel).toBeVisible({ timeout: 3_000 });

  // Click the "worker" node in the canvas
  await page.getByText("worker", { exact: true }).first().click();

  // The panel should auto-close
  await expect(infoPanel).not.toBeVisible({ timeout: 3_000 });
});

test("YAML tab shows serialized pipeline and updates on mutation (#69)", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  // Enter edit mode and open the pipeline
  await page.locator('[title="Toggle edit mode"]').click();
  await page.getByRole("button", { name: new RegExp(PIPELINE_NAME) }).click();

  // Open the pipeline info panel
  const infoBtn = page.getByTestId("toolbar-info");
  await expect(infoBtn).toBeVisible({ timeout: 3_000 });
  await infoBtn.click();

  const infoPanel = page.getByTestId("pipeline-info-panel");
  await expect(infoPanel).toBeVisible({ timeout: 3_000 });

  // Click the YAML tab
  const yamlTab = page.getByTestId("info-tab-yaml");
  await expect(yamlTab).toBeVisible();
  await yamlTab.click();

  // Verify YAML content is visible and contains pipeline data
  const yamlView = page.getByTestId("info-yaml-content");
  await expect(yamlView).toBeVisible({ timeout: 3_000 });
  await expect(yamlView).toContainText(PIPELINE_NAME);
  await expect(yamlView).toContainText("worker");

  // Close the panel so we can add a node
  await page.getByTestId("info-panel-close").click();
  await expect(infoPanel).not.toBeVisible();

  // Add a new node via toolbar
  await page.getByTestId("toolbar-add").click();
  await page.waitForTimeout(500);

  // Re-open the info panel and check YAML tab reflects the new node
  await infoBtn.click();
  await expect(infoPanel).toBeVisible({ timeout: 3_000 });
  await yamlTab.click();
  await expect(yamlView).toContainText("node-", { timeout: 3_000 });
});
