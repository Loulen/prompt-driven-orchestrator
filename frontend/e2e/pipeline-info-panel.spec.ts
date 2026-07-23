import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { openPipelineForEdit } from "./helpers";
import type { Page } from "@playwright/test";

// The dirty edit canvas raises an "External edit conflict" modal when the
// pipeline file changes on disk while the tab has unsaved edits. Cross-test
// file-watch noise can trip it during a full-suite run; dismiss it (keep
// canvas) so it does not intercept toolbar clicks. No-op when absent.
async function dismissConflictIfPresent(page: Page): Promise<void> {
  const backdrop = page.getByTestId("conflict-modal-backdrop");
  if (await backdrop.isVisible().catch(() => false)) {
    await page.keyboard.press("Escape");
    await expect(backdrop).not.toBeVisible({ timeout: 2_000 });
  }
}

// Layer 3b — Pipeline info panel (refs #56, #69, #385, ADR 0004).
// Verifies:
// 1. Clicking the toolbar `i` icon opens the PipelineInfoPanel.
// 2. The panel shows pipeline metadata (name).
// 3. During a Run, a terminal element is present for the manager session.
// 4. Clicking a node in the canvas auto-closes the info panel (#69).
// 5. YAML tab shows serialized pipeline and updates on canvas mutation (#69).
// 6. Selecting a DIFFERENT run auto-closes the info panel (#385), including two
//    runs of the same pipeline; reselecting the already-active run is a no-op.
// 7. Selecting a Trigger closes the info panel and surfaces the Trigger detail
//    from behind it (#385 — info outranks trigger in rightPaneOwner until closed).

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-info-panel-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);

// Post-refonte the parser requires exactly one start node (zero inputs, one
// output named `user_prompt`) and one end node (zero outputs, one input named
// `result`). start → worker → end is the minimal valid chain.
const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
variables:
  max_iter:
    type: int
    default: 3
nodes:
  - id: start
    name: Start
    type: start
    inputs: []
    outputs:
      - name: user_prompt
    view: { x: 0, y: 100 }
  - id: worker
    name: worker
    type: doc-only
    inputs:
      - name: in
    outputs:
      - name: out
    view: { x: 200, y: 100 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
    outputs: []
    view: { x: 400, y: 100 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: in }
  - source: { node: worker, port: out }
    target: { node: end, port: result }
`;

test.beforeAll(async () => {
  process.env.PDO_TMUX_CMD_OVERRIDE = 'exec sh -c "cat"';
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  delete process.env.PDO_TMUX_CMD_OVERRIDE;
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
    multipart: {
      pipeline: PIPELINE_NAME,
      input: "e2e info panel test",
    },
  });
  expect(resp.status()).toBe(201);
  const { run_id } = await resp.json();

  // Select the run in the left panel
  await page.getByText(run_id.slice(0, 20)).first().click({ timeout: 5_000, position: { x: 5, y: 5 } });
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

  // The manager terminal lives under the Manager tab (post-refonte: the info
  // panel is tabbed Info | Manager | YAML; Info is the default). Switch to it.
  const managerTab = page.getByTestId("info-tab-manager");
  await expect(managerTab).toBeVisible({ timeout: 3_000 });
  await managerTab.click();

  // Assert the manager terminal is rendered (run is active)
  const terminal = infoPanel.getByTestId("tmux-terminal");
  await expect(terminal).toBeVisible({ timeout: 5_000 });

  // Close the panel
  await page.getByTestId("info-panel-close").click();
  await expect(infoPanel).not.toBeVisible();

  // Cleanup tmux sessions
  const { execSync } = await import("node:child_process");
  try {
    execSync(`tmux kill-session -t pdo-${run_id}-worker-iter-1`, {
      stdio: "ignore",
    });
  } catch {
    // ok
  }
  try {
    execSync(`tmux kill-session -t pdo-mgr-${run_id}`, {
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

  // Open the pipeline from the Library tab (post-refonte: no edit toggle).
  await openPipelineForEdit(page, PIPELINE_NAME);

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

  // Open the pipeline from the Library tab (post-refonte: no edit toggle).
  await openPipelineForEdit(page, PIPELINE_NAME);

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

  // Add a new node via toolbar. Since #307/#310 `toolbar-add` is a dropdown
  // (Node | Note) — open it, then pick "Node" (`add-menu-node`), which inserts a
  // `code-mutating` node whose default name is "implementer" (node ids are random
  // nanoids, no longer `node-N`).
  await dismissConflictIfPresent(page);
  await page.getByTestId("toolbar-add").click();
  await page.getByTestId("add-menu-node").click();
  await page.waitForTimeout(500);

  // Re-open the info panel and check YAML tab reflects the new node
  await dismissConflictIfPresent(page);
  await infoBtn.click();
  await expect(infoPanel).toBeVisible({ timeout: 3_000 });
  await yamlTab.click();
  await expect(yamlView).toContainText("implementer", { timeout: 3_000 });
});

test("library tab after a run: panel shows template, not the previous run", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  // Step 1 — create + select a run so selectedRun is populated.
  const resp = await page.request.post(`${baseURL}/runs`, {
    multipart: { pipeline: PIPELINE_NAME, input: "library-tab regression" },
  });
  expect(resp.status()).toBe(201);
  const { run_id } = await resp.json();

  await page.getByText(run_id.slice(0, 20)).first().click({ timeout: 5_000, position: { x: 5, y: 5 } });
  await page.waitForTimeout(500);

  // Sanity — open info panel on the run tab and confirm run metadata is shown.
  const infoBtn = page.getByTestId("toolbar-info");
  await expect(infoBtn).toBeVisible({ timeout: 3_000 });
  await infoBtn.click();

  const infoPanel = page.getByTestId("pipeline-info-panel");
  await expect(infoPanel).toBeVisible({ timeout: 3_000 });
  await expect(infoPanel).toContainText(`run ${run_id.slice(-8)}`);
  await expect(page.getByTestId("info-tab-manager")).toBeVisible();

  // Close the panel before switching tabs.
  await page.getByTestId("info-panel-close").click();
  await expect(infoPanel).not.toBeVisible();

  // Step 2 — switch to a library template tab (any non-run scope). The run row
  // in the Runs tab also renders the pipeline name (`run-pipeline-name`), so we
  // must go through the Library tab to open the template, not match raw text.
  await openPipelineForEdit(page, PIPELINE_NAME);
  await page.waitForTimeout(500);

  // Step 3 — open the info panel on the library tab.
  await dismissConflictIfPresent(page);
  await infoBtn.click();
  await expect(infoPanel).toBeVisible({ timeout: 3_000 });

  // Regression checks: no leakage of the previous run into the panel.
  await expect(infoPanel).toContainText("template ·");
  await expect(infoPanel).not.toContainText(`run ${run_id.slice(-8)}`);
  await expect(page.getByTestId("info-tab-manager")).toHaveCount(0);

  // Cleanup tmux sessions
  const { execSync } = await import("node:child_process");
  for (const session of [
    `pdo-${run_id}-worker-iter-1`,
    `pdo-mgr-${run_id}`,
  ]) {
    try {
      execSync(`tmux kill-session -t ${session}`, { stdio: "ignore" });
    } catch {
      // ok
    }
  }
});

// Shared tmux cleanup for a run created by the #385 cases below.
async function killRunSessions(runId: string): Promise<void> {
  const { execSync } = await import("node:child_process");
  for (const session of [`pdo-${runId}-worker-iter-1`, `pdo-mgr-${runId}`]) {
    try {
      execSync(`tmux kill-session -t ${session}`, { stdio: "ignore" });
    } catch {
      // ok
    }
  }
}

test("selecting a different run closes the pipeline info panel (#385)", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  // Two runs of the SAME pipeline — run tabs are keyed __run__<id>, so switching
  // between them still moves activeTabId (the make-or-break edge case for #385).
  const mkRun = async (): Promise<string> => {
    const r = await page.request.post(`${baseURL}/runs`, {
      multipart: { pipeline: PIPELINE_NAME, input: "e2e #385" },
    });
    expect(r.status()).toBe(201);
    return (await r.json()).run_id as string;
  };
  const runA = await mkRun();
  const runB = await mkRun();

  // Select run A and open the info overlay on it.
  await page
    .getByText(runA.slice(0, 20))
    .first()
    .click({ timeout: 5_000, position: { x: 5, y: 5 } });
  await page.waitForTimeout(500);
  const infoBtn = page.getByTestId("toolbar-info");
  await expect(infoBtn).toBeVisible({ timeout: 3_000 });
  await infoBtn.click();
  const infoPanel = page.getByTestId("pipeline-info-panel");
  await expect(infoPanel).toBeVisible({ timeout: 3_000 });

  // Select the OTHER run → the overlay must auto-close (#385).
  await page
    .getByText(runB.slice(0, 20))
    .first()
    .click({ timeout: 5_000, position: { x: 5, y: 5 } });
  await expect(infoPanel).not.toBeVisible({ timeout: 3_000 });

  // Reselecting the SAME (now-active) run must NOT reopen the overlay (no-op):
  // activeTabId is unchanged, so #385 does not fire.
  await page
    .getByText(runB.slice(0, 20))
    .first()
    .click({ timeout: 5_000, position: { x: 5, y: 5 } });
  await page.waitForTimeout(300);
  await expect(infoPanel).not.toBeVisible();

  await killRunSessions(runA);
  await killRunSessions(runB);
});

test("selecting a Trigger surfaces its detail from behind the open info panel (#385)", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  // Seed a Trigger for PIPELINE_NAME. `input_template` is required because the
  // seeded pipeline is prompt_required by default (a cron-only trigger would be
  // rejected). cron never fires within the test window.
  const trigResp = await page.request.post(`${baseURL}/triggers`, {
    data: {
      name: `e2e-385-${process.pid}`,
      pipeline_id: PIPELINE_NAME,
      cron: "0 0 1 1 *",
      input_template: "e2e #385 trigger",
    },
  });
  expect(trigResp.status()).toBe(201);
  const triggerId = (await trigResp.json()).id as string;

  // A run gives the info overlay a tab (__run__<id>) that is DIFFERENT from the
  // Trigger's pipeline template tab, so selecting the Trigger genuinely moves
  // activeTabId (avoids the "pipeline already active" no-op).
  const runResp = await page.request.post(`${baseURL}/runs`, {
    multipart: { pipeline: PIPELINE_NAME, input: "e2e #385 trigger-run" },
  });
  expect(runResp.status()).toBe(201);
  const runId = (await runResp.json()).run_id as string;

  // Reload so the mount refresh picks up the seeded trigger + run.
  await page.reload();
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  // Open the info overlay on the run tab.
  await page
    .getByText(runId.slice(0, 20))
    .first()
    .click({ timeout: 5_000, position: { x: 5, y: 5 } });
  await page.waitForTimeout(500);
  const infoBtn = page.getByTestId("toolbar-info");
  await expect(infoBtn).toBeVisible({ timeout: 3_000 });
  await infoBtn.click();
  const infoPanel = page.getByTestId("pipeline-info-panel");
  await expect(infoPanel).toBeVisible({ timeout: 3_000 });

  // Switch the LEFT panel to the Triggers tab (does NOT touch activeTabId, so
  // the overlay stays open) and click the Trigger row.
  await page.getByRole("tab", { name: "Triggers" }).click();
  const triggerRow = page.getByTestId("trigger-row").first();
  await expect(triggerRow).toBeVisible({ timeout: 3_000 });
  await triggerRow.click();

  // The overlay must close AND the Trigger detail must surface (before the fix
  // the detail stayed hidden behind the overlay — info outranks trigger).
  await expect(infoPanel).not.toBeVisible({ timeout: 3_000 });
  await expect(page.getByTestId("trigger-detail-panel")).toBeVisible({
    timeout: 3_000,
  });

  await page.request.delete(`${baseURL}/triggers/${triggerId}`);
  await killRunSessions(runId);
});
