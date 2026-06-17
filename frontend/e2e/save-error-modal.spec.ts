import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Layer 3b — Save error modal (#74, refs #1).
// Verifies:
// 1. Stubbing the daemon PUT endpoint to return an error → modal appears.
// 2. Modal shows the error message.
// 3. Clicking "Voir le YAML" opens the info panel on the YAML tab.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-save-err-${process.pid}-${Date.now()}`;
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

test.beforeAll(async () => {
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await fs.rm(PROMPTS_DIR, { recursive: true, force: true });
});

test("save error shows modal, Voir le YAML opens YAML tab", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  // Open pipeline in edit mode
  await page.locator('[title="Toggle edit mode"]').click();
  await page.getByRole("button", { name: new RegExp(PIPELINE_NAME) }).click();

  const tabTitle = page.getByTestId(`tab-title-${PIPELINE_NAME}`);
  await expect(tabTitle).toBeVisible();

  // Make a change to dirty the tab
  await page.getByText("alpha", { exact: true }).first().click();
  const promptArea = page.getByPlaceholder("Enter the node's role prompt...");
  await expect(promptArea).toBeVisible();
  await promptArea.fill("DIRTY_SAVE_ERROR");

  await expect(tabTitle).toHaveText(`• ${PIPELINE_NAME}.yaml`);

  // Intercept the PUT endpoint to return a structured error
  await page.route("**/pipelines/**", (route) => {
    if (route.request().method() === "PUT") {
      return route.fulfill({
        status: 400,
        contentType: "application/json",
        body: JSON.stringify({
          error: "invalid YAML: missing field 'name'",
          message: "missing field 'name'",
          line: 3,
        }),
      });
    }
    return route.continue();
  });

  // Click Save
  const saveBtn = page.getByTestId("save-button");
  await saveBtn.click();

  // Assert save error modal appears
  const modal = page.getByTestId("save-error-modal");
  await expect(modal).toBeVisible({ timeout: 3_000 });

  // Assert modal shows error message
  const errorMsg = page.getByTestId("save-error-message");
  await expect(errorMsg).toContainText("missing field");

  // Assert the modal title
  await expect(modal).toContainText("Impossible de sauvegarder");

  // Click "Voir le YAML"
  const viewYamlBtn = page.getByTestId("save-error-view-yaml");
  await viewYamlBtn.click();

  // Modal should disappear
  await expect(modal).not.toBeVisible();

  // Pipeline info panel should appear on YAML tab
  const infoPanel = page.getByTestId("pipeline-info-panel");
  await expect(infoPanel).toBeVisible({ timeout: 3_000 });

  const yamlTab = page.getByTestId("info-tab-yaml");
  await expect(yamlTab).toHaveClass(/border-acc/);

  // YAML content should be visible
  const yamlContent = page.getByTestId("info-yaml-content");
  await expect(yamlContent).toBeVisible();
});

test("save error modal dismiss closes modal without opening info panel", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  await page.locator('[title="Toggle edit mode"]').click();
  await page.getByRole("button", { name: new RegExp(PIPELINE_NAME) }).click();

  const tabTitle = page.getByTestId(`tab-title-${PIPELINE_NAME}`);
  await expect(tabTitle).toBeVisible();

  await page.getByText("alpha", { exact: true }).first().click();
  const promptArea = page.getByPlaceholder("Enter the node's role prompt...");
  await expect(promptArea).toBeVisible();
  await promptArea.fill("DIRTY_DISMISS_TEST");

  await page.route("**/pipelines/**", (route) => {
    if (route.request().method() === "PUT") {
      return route.fulfill({
        status: 400,
        contentType: "application/json",
        body: JSON.stringify({
          error: "invalid YAML: syntax error",
          message: "syntax error",
        }),
      });
    }
    return route.continue();
  });

  await page.getByTestId("save-button").click();

  const modal = page.getByTestId("save-error-modal");
  await expect(modal).toBeVisible({ timeout: 3_000 });

  // Click dismiss
  await page.getByTestId("save-error-dismiss").click();

  // Modal should disappear
  await expect(modal).not.toBeVisible();

  // Info panel should NOT be open
  await expect(page.getByTestId("pipeline-info-panel")).not.toBeVisible();
});
