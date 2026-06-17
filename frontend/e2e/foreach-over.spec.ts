import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Layer 3b — ForEach `over` dropdown E2E (refs #65).
// Seeds a pipeline with a ForEach node connected to an upstream node with a
// typed `list` output. Verifies the `over` dropdown is enabled, lists the
// field, persists on save, and that the lint diagnostic appears when unset.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-fe-over-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);
const PROMPTS_DIR = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.prompts`);

const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: upstream
    name: upstream
    type: doc-only
    inputs:
      - name: in
        side: left
    outputs:
      - name: out
        side: right
        frontmatter:
          issues:
            type: list
    view: { x: 100, y: 200 }
  - id: fe1
    name: per-item
    type: for-each
    inputs:
      - name: in
        side: left
      - name: break
        side: left
    outputs:
      - name: body
        side: right
      - name: done
        side: right
    view: { x: 400, y: 200 }
  - id: worker
    name: worker
    type: doc-only
    inputs:
      - name: in
        side: left
    outputs:
      - name: out
        side: right
    view: { x: 700, y: 200 }
edges:
  - source: { node: upstream, port: out }
    target: { node: fe1, port: in }
  - source: { node: fe1, port: body }
    target: { node: worker, port: in }
  - source: { node: worker, port: out }
    target: { node: fe1, port: done }
`;

test.beforeAll(async () => {
  await fs.mkdir(PROMPTS_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
  await fs.writeFile(
    path.join(PROMPTS_DIR, "upstream.md"),
    "Produce issues.\n",
  );
  await fs.writeFile(
    path.join(PROMPTS_DIR, "worker.md"),
    "Process one issue.\n",
  );
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await fs.rm(PROMPTS_DIR, { recursive: true, force: true });
});

test("over dropdown lists upstream list fields and persists on save", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  // Enter edit mode and open pipeline
  await page.locator("[data-testid='edit-toggle']").click();
  await page.getByText(PIPELINE_NAME).first().click({ timeout: 5_000 });
  await page.waitForTimeout(500);

  // Select the ForEach node to open its inspector
  await page.getByText("per-item", { exact: true }).first().click();
  await page.waitForTimeout(300);

  // The over dropdown should be visible and enabled
  const overSelect = page.getByTestId("foreach-over-select");
  await expect(overSelect).toBeVisible({ timeout: 3_000 });
  await expect(overSelect).toBeEnabled();

  // Dropdown should contain the "issues" option from upstream
  const issuesOption = overSelect.locator("option[value='issues']");
  await expect(issuesOption).toHaveCount(1);

  // Select "issues"
  await overSelect.selectOption("issues");

  // Save
  const saveBtn = page.getByTestId("save-button");
  await expect(saveBtn).toBeEnabled({ timeout: 2_000 });
  await saveBtn.click();
  await expect(saveBtn).toBeDisabled({ timeout: 3_000 });

  // Reload and reopen to verify persistence
  await page.reload();
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });
  await page.locator("[data-testid='edit-toggle']").click();
  await page.getByText(PIPELINE_NAME).first().click({ timeout: 5_000 });
  await page.waitForTimeout(500);

  // Select the ForEach node again
  await page.getByText("per-item", { exact: true }).first().click();
  await page.waitForTimeout(300);

  // Verify the dropdown retained the "issues" value
  const overSelectReloaded = page.getByTestId("foreach-over-select");
  await expect(overSelectReloaded).toBeVisible({ timeout: 3_000 });
  await expect(overSelectReloaded).toHaveValue("issues");
});

test("lint diagnostic appears when over is unset on wired ForEach", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  // Enter edit mode and open pipeline
  await page.locator("[data-testid='edit-toggle']").click();
  await page.getByText(PIPELINE_NAME).first().click({ timeout: 5_000 });
  await page.waitForTimeout(500);

  // The ForEach node has no `over` set in the seed YAML, and it has an
  // `in` edge wired → the lint banner should show a diagnostic.
  const lintBanner = page.getByTestId("lint-banner");
  await expect(lintBanner).toBeVisible({ timeout: 3_000 });
  await expect(
    lintBanner.getByText(/no "over" field set/i),
  ).toBeVisible();
});
