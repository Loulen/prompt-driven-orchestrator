import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { runMultipart } from "./helpers";

// Layer 3b — a run opens on its canvas automatically (issue #57), in
// « pilotage » (ADR-0080): read-only until « Edit › Edit for this run », which
// brings the edit palette back; « Finish editing » locks it again.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-unified-edit-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);

const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: planner
    name: planner
    type: agent
    isolated_worktree: false
    inputs:
      - name: task
    outputs:
      - name: plan
    view: { x: 100, y: 100 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: planner, port: task }
`;

test.beforeAll(async () => {
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
});

// POST /runs is multipart/form-data post-refonte (JSON 400s). `variables` must
// be a JSON string in the form. The run entry in the Runs list renders the
// pipeline_name (full) and a truncated run_id (`run_id.slice(0, 20)`), so we
// select the run by its unique pipeline name rather than the full id.
async function createRun(
  request: import("@playwright/test").APIRequestContext,
): Promise<string> {
  const resp = await request.post("/runs", {
    multipart: runMultipart({ pipeline: PIPELINE_NAME, input: "test input", variables: "{}" }),
  });
  expect(resp.ok()).toBeTruthy();
  const { run_id } = await resp.json();
  return run_id;
}

test("unified edit mode: selecting a run opens editor canvas automatically", async ({ page, request }) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  // Create a run via the (multipart) API
  const run_id = await createRun(request);

  // Wait for the run to appear in the Runs list (default tab) and select it.
  const runEntry = page.getByText(PIPELINE_NAME).first();
  await expect(runEntry).toBeVisible({ timeout: 5_000 });
  await runEntry.click();

  // Editor canvas should open automatically — the EditCanvas always mounts its
  // EditToolbar. The tab bar appears too once the run-scoped tab is open.
  await expect(page.getByTestId("tab-list")).toBeVisible({ timeout: 5_000 });
  await expect(page.getByTestId("edit-toolbar")).toBeVisible();
  await expect(page.getByTestId(`tab-title-__run__${run_id}`)).toBeVisible();

  // ADR-0080: following a run never modifies it — no authoring palette.
  await expect(page.getByTestId("toolbar-add")).toHaveCount(0);
  await expect(page.getByTestId("toolbar-undo")).toHaveCount(0);
  await expect(page.getByTestId("toolbar-edit")).toBeVisible();
});

test("Edit for this run unlocks the canvas, Finish editing locks it again", async ({ page, request }) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  await createRun(request);

  const runEntry = page.getByText(PIPELINE_NAME).first();
  await expect(runEntry).toBeVisible({ timeout: 5_000 });
  await runEntry.click();

  await expect(page.getByTestId("edit-toolbar")).toBeVisible({ timeout: 5_000 });
  await page.getByTestId("toolbar-edit").click();
  await page.getByTestId("toolbar-edit-for-run").click();
  await expect(page.getByTestId("toolbar-add")).toBeVisible();
  await expect(page.getByTestId("save-button")).toHaveText(/Save for this run/);

  await page.getByTestId("toolbar-finish-editing").click();
  await expect(page.getByTestId("toolbar-add")).toHaveCount(0);
  await expect(page.getByTestId("toolbar-edit")).toBeVisible();
});
