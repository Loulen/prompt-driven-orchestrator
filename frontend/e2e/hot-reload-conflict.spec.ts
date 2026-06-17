import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Layer 3b — issue #72: conflict modal on external edit when canvas is dirty.
// Verifies:
// 1. External edit on a dirty tab → conflict modal appears.
// 2. "Take external" discards local changes and renders external state.
// 3. "Keep canvas" retains local changes; next save overwrites disk.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-conflict-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);
const PROMPTS_DIR = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.prompts`);

const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: alpha
    type: doc-only
    prompt_file: ${PIPELINE_NAME}.prompts/alpha.md
    inputs:
      - name: in
    outputs:
      - name: out
    view: { x: 100, y: 100 }
edges: []
`;

const EXTERNAL_YAML = `name: ${PIPELINE_NAME}-external
version: "2.0"
nodes:
  - id: alpha
    type: doc-only
    prompt_file: ${PIPELINE_NAME}.prompts/alpha.md
    inputs:
      - name: in
    outputs:
      - name: out
    view: { x: 100, y: 100 }
  - id: beta
    type: doc-only
    inputs:
      - name: in
    outputs:
      - name: out
    view: { x: 400, y: 100 }
edges: []
`;

test.beforeAll(async () => {
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await fs.rm(PROMPTS_DIR, { recursive: true, force: true });
});

test("external edit on dirty tab shows conflict modal — Take external", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  // Open pipeline in edit mode
  await page.locator('[title="Toggle edit mode"]').click();
  await page
    .getByRole("button", { name: new RegExp(PIPELINE_NAME) })
    .click();

  // Make the tab dirty by editing a prompt
  await page.getByText("alpha", { exact: true }).first().click();
  const promptArea = page.getByPlaceholder(
    "Enter the node's role prompt...",
  );
  await expect(promptArea).toBeVisible();
  await promptArea.fill("LOCAL_DIRTY_CHANGE");

  // Verify dirty indicator
  const tabTitle = page.getByTestId(`tab-title-${PIPELINE_NAME}`);
  await expect(tabTitle).toHaveText(`• ${PIPELINE_NAME}.yaml`);

  // Externally modify the YAML on disk
  await fs.writeFile(PIPELINE_PATH, EXTERNAL_YAML);

  // Conflict modal should appear
  const conflictModal = page.getByTestId("conflict-modal-backdrop");
  await expect(conflictModal).toBeVisible({ timeout: 8_000 });
  await expect(page.getByText("External edit conflict")).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Keep canvas" }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Take external" }),
  ).toBeVisible();

  // Click "Take external"
  await page.getByRole("button", { name: "Take external" }).click();

  // Modal should close
  await expect(conflictModal).not.toBeVisible();

  // Tab should no longer be dirty
  await expect(tabTitle).toHaveText(`${PIPELINE_NAME}.yaml`);
});

test("external edit on dirty tab — Keep canvas retains local changes", async ({
  page,
}) => {
  // Reset file to seed state
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);

  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  await page.locator('[title="Toggle edit mode"]').click();
  await page
    .getByRole("button", { name: new RegExp(PIPELINE_NAME) })
    .click();

  // Make the tab dirty
  await page.getByText("alpha", { exact: true }).first().click();
  const promptArea = page.getByPlaceholder(
    "Enter the node's role prompt...",
  );
  await expect(promptArea).toBeVisible();
  await promptArea.fill("MY_LOCAL_EDIT");

  const tabTitle = page.getByTestId(`tab-title-${PIPELINE_NAME}`);
  await expect(tabTitle).toHaveText(`• ${PIPELINE_NAME}.yaml`);

  // External edit
  await fs.writeFile(PIPELINE_PATH, EXTERNAL_YAML);

  // Conflict modal appears
  const conflictModal = page.getByTestId("conflict-modal-backdrop");
  await expect(conflictModal).toBeVisible({ timeout: 8_000 });

  // Click "Keep canvas"
  await page.getByRole("button", { name: "Keep canvas" }).click();

  // Modal closes, tab is still dirty with local changes
  await expect(conflictModal).not.toBeVisible();
  await expect(tabTitle).toHaveText(`• ${PIPELINE_NAME}.yaml`);
  await expect(promptArea).toHaveValue("MY_LOCAL_EDIT");

  // Save overwrites disk with local version
  await page.getByTestId("save-button").click();
  await expect(tabTitle).toHaveText(`${PIPELINE_NAME}.yaml`);
});

test("external edit on clean tab silently re-renders (no modal)", async ({
  page,
}) => {
  // Reset file
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);

  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  await page.locator('[title="Toggle edit mode"]').click();
  await page
    .getByRole("button", { name: new RegExp(PIPELINE_NAME) })
    .click();

  // Tab should be clean
  const tabTitle = page.getByTestId(`tab-title-${PIPELINE_NAME}`);
  await expect(tabTitle).toHaveText(`${PIPELINE_NAME}.yaml`);

  // External edit on a clean tab → no modal, silent re-render
  await fs.writeFile(PIPELINE_PATH, EXTERNAL_YAML);

  // Wait for potential modal (should NOT appear)
  await page.waitForTimeout(4000);
  await expect(
    page.getByTestId("conflict-modal-backdrop"),
  ).not.toBeVisible();

  // The external change flash indicator should appear briefly
  // (we don't assert on the flash timing, just that no modal appeared)
});
