import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as os from "node:os";
import * as path from "node:path";
import { confirmSaveVersion, openPipelineForEdit } from "./helpers";

// Layer 4 — Save button + dirty indicator (#35).
// Verifies:
// 1. Editing a pipeline marks the tab unsaved (its dot).
// 2. Clicking Save (through the « Save… » window, ADR-0088) clears the dot and
//    shows "Saved Xs ago".
// A launch never saves silently (ADR-0088): see pipeline-versions.spec.ts.

const PIPELINE_NAME = `e2e-save-btn-${process.pid}-${Date.now()}`;
// The instance pipelines the daemon watches.
const PIPELINE_DIR = path.join(os.homedir(), ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);
const PROMPTS_DIR = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.prompts`);

// Post-refonte schema (ADR-0011): a valid pipeline needs exactly one `start`
// (zero inputs, one `user_prompt` output) and one `end` (one `result` input,
// zero outputs), plus a `name` on every node — otherwise the daemon rejects
// the YAML (400) and the edit canvas never opens.
const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - { name: user_prompt, side: right }
    view: { x: 0, y: 100 }
  - id: alpha
    name: alpha
    type: agent
    isolated_worktree: false
    prompt_file: ${PIPELINE_NAME}.prompts/alpha.md
    inputs:
      - { name: in, side: left }
    outputs:
      - { name: out, side: right }
    view: { x: 200, y: 100 }
  - id: end
    name: End
    type: end
    inputs:
      - { name: result, side: left }
    view: { x: 400, y: 100 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: alpha, port: in }
  - source: { node: alpha, port: out }
    target: { node: end, port: result }
`;

test.beforeAll(async () => {
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await fs.rm(PROMPTS_DIR, { recursive: true, force: true });
});

test("edit triggers dirty dot, Save clears it and shows relative time", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  // Open the pipeline into the edit canvas
  await openPipelineForEdit(page, PIPELINE_NAME);

  // Tab should be clean — no unsaved dot
  const tabTitle = page.getByTestId(`tab-title-${PIPELINE_NAME}`);
  await expect(tabTitle).toBeVisible();
  await expect(tabTitle).toHaveText(`${PIPELINE_NAME}.yaml`);
  await expect(tabTitle.locator("xpath=ancestor::button[1]")).not.toHaveAttribute("data-dirty", "true");

  // Save button should be disabled (nothing dirty)
  const saveBtn = page.getByTestId("save-button");
  await expect(saveBtn).toBeDisabled();

  // Click the node to open inspector, then edit the prompt
  await page.getByText("alpha", { exact: true }).first().click();
  const promptArea = page.getByPlaceholder("Enter the node's role prompt...");
  await expect(promptArea).toBeVisible();
  await promptArea.fill("DIRTY_EDIT");

  // Tab title should now carry the unsaved dot
  await expect(tabTitle.locator("xpath=ancestor::button[1]")).toHaveAttribute("data-dirty", "true");

  // Save button should be enabled
  await expect(saveBtn).toBeEnabled();

  // Click Save
  await saveBtn.click();
  await confirmSaveVersion(page);

  // • prefix should disappear
  await expect(tabTitle).toHaveText(`${PIPELINE_NAME}.yaml`);
  await expect(tabTitle.locator("xpath=ancestor::button[1]")).not.toHaveAttribute("data-dirty", "true");

  // "Saved Xs ago" should appear
  const savedAgo = page.getByTestId("saved-ago");
  await expect(savedAgo).toBeVisible({ timeout: 2_000 });
  await expect(savedAgo).toHaveText(/Saved/);

  // Save button should be disabled again
  await expect(saveBtn).toBeDisabled();
});

test("Cmd+S saves the active tab", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  await openPipelineForEdit(page, PIPELINE_NAME);

  const tabTitle = page.getByTestId(`tab-title-${PIPELINE_NAME}`);
  await expect(tabTitle).toBeVisible();

  await page.getByText("alpha", { exact: true }).first().click();
  const promptArea = page.getByPlaceholder("Enter the node's role prompt...");
  await expect(promptArea).toBeVisible();
  await promptArea.fill("DIRTY_VIA_KB");

  await expect(tabTitle.locator("xpath=ancestor::button[1]")).toHaveAttribute("data-dirty", "true");

  // Ctrl+S (cross-platform for Playwright)
  await page.keyboard.press("Control+s");
  await confirmSaveVersion(page);

  // Should clear dirty prefix
  await expect(tabTitle).toHaveText(`${PIPELINE_NAME}.yaml`);
  await expect(tabTitle.locator("xpath=ancestor::button[1]")).not.toHaveAttribute("data-dirty", "true");
  await expect(page.getByTestId("saved-ago")).toBeVisible({ timeout: 2_000 });
});
