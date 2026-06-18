import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Layer 3b — proves issue #57 unified edit mode. Boots the daemon, creates a
// run, asserts the editor canvas appears automatically (no "Edit this run"
// toggle needed), verifies the edit palette is present and no pencil toggle
// exists anywhere.

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
    type: doc-only
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
    multipart: { pipeline: PIPELINE_NAME, input: "test input", variables: "{}" },
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

  // Editor canvas should open automatically — the post-refonte EditCanvas
  // always mounts its EditToolbar (no separate "Edit this run" step). The tab
  // bar appears too once the run-scoped edit tab is open.
  await expect(page.getByTestId("tab-list")).toBeVisible({ timeout: 5_000 });
  await expect(page.getByTestId("edit-toolbar")).toBeVisible();
  await expect(page.getByTestId("toolbar-add")).toBeVisible();

  // The opened tab is run-scoped: its id is `__run__<run_id>`, so the editor we
  // see is editing the run's pipeline (run-scoped edits "sync to template").
  // On a live run a node auto-selects, so the RunInfoSidebar footnote is not a
  // reliable signal here — the run-scoped tab id is.
  await expect(page.getByTestId(`tab-title-__run__${run_id}`)).toBeVisible();
});

test("no pencil toggle or edit-this-run button exists", async ({ page, request }) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  // Create a run and select it
  await createRun(request);

  const runEntry = page.getByText(PIPELINE_NAME).first();
  await expect(runEntry).toBeVisible({ timeout: 5_000 });
  await runEntry.click();

  // Wait for the editor to load (toolbar present)
  await expect(page.getByTestId("edit-toolbar")).toBeVisible({ timeout: 5_000 });

  // "Edit this run" button should NOT exist
  await expect(page.getByRole("button", { name: "Edit this run" })).toHaveCount(0);

  // No pencil toggle should exist in the toolbar
  await expect(page.getByTitle("Toggle edit mode")).toHaveCount(0);
});
