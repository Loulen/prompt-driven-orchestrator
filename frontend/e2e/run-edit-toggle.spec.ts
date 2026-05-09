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
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".maestro", "pipelines");
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

test("unified edit mode: selecting a run opens editor canvas automatically", async ({ page, request }) => {
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

  // Editor canvas should open automatically — AddPalette buttons visible
  await expect(page.getByRole("button", { name: "code" })).toBeVisible({ timeout: 5_000 });
  await expect(page.getByRole("button", { name: "doc" })).toBeVisible();

  // The run-scoped footnote should say "changes sync to template"
  await expect(page.getByText("changes sync to template")).toBeVisible();
});

test("no pencil toggle or edit-this-run button exists", async ({ page, request }) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  // Create a run and select it
  const resp = await request.post("/runs", {
    data: { pipeline: PIPELINE_NAME, input: "test input", variables: {} },
  });
  expect(resp.ok()).toBeTruthy();
  const { run_id } = await resp.json();

  const runEntry = page.getByText(run_id).first();
  await expect(runEntry).toBeVisible({ timeout: 5_000 });
  await runEntry.click();

  // Wait for editor to load
  await expect(page.getByRole("button", { name: "code" })).toBeVisible({ timeout: 5_000 });

  // "Edit this run" button should NOT exist
  await expect(page.getByRole("button", { name: "Edit this run" })).not.toBeVisible();

  // No pencil toggle should exist in the toolbar
  await expect(page.getByTitle("Toggle edit mode")).not.toBeVisible();
});
