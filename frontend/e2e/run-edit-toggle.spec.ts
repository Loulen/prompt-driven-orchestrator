import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Layer 3b — proves issue #28 run-mode authoring toggle. Boots the daemon,
// creates a run, opens the "Edit this run" overlay, asserts that the editor
// canvas + inspector appear (with AddPalette and run-scoped footnote), then
// stops editing and asserts the run view is restored.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-run-edit-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".maestro", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);

const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: planner
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: plan
    view: { x: 100, y: 100 }
edges: []
`;

test.beforeAll(async () => {
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
});

test("edit-this-run toggle swaps to editor and back", async ({ page, request }) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  // Create a run via the API
  const resp = await request.post("/runs", {
    data: { pipeline: PIPELINE_NAME, input: "test input", variables: {} },
  });
  expect(resp.ok()).toBeTruthy();
  const { run_id } = await resp.json();

  // Wait for the run to appear in the sidebar and select it
  const runEntry = page.getByText(run_id).first();
  await expect(runEntry).toBeVisible({ timeout: 5_000 });
  await runEntry.click();

  // The run overlay should show the pipeline name
  await expect(page.getByText(PIPELINE_NAME).first()).toBeVisible();

  // Click "Edit this run"
  const editButton = page.getByRole("button", { name: "Edit this run" });
  await expect(editButton).toBeVisible();
  await editButton.click();

  // Should see the AddPalette (code + doc buttons)
  await expect(page.getByRole("button", { name: "code" })).toBeVisible({ timeout: 3_000 });
  await expect(page.getByRole("button", { name: "doc" })).toBeVisible();

  // Should see the run-scoped footnote
  await expect(page.getByText("template unchanged")).toBeVisible();

  // Should see "Stop editing" button
  const stopButton = page.getByRole("button", { name: "Stop editing" });
  await expect(stopButton).toBeVisible();

  // Click stop editing
  await stopButton.click();

  // The run overlay should be back with "Edit this run" visible again
  await expect(page.getByRole("button", { name: "Edit this run" })).toBeVisible({ timeout: 3_000 });

  // The AddPalette should be gone
  await expect(page.getByRole("button", { name: "code" })).not.toBeVisible();
});
