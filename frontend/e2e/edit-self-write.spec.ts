import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Layer 3b — UI witness for Bug E (#17). Without the daemon-side fix, edits
// would be wiped by the broadcast/reload cycle: the daemon's PUT triggers a
// watcher event, the frontend reloads from disk, and any keystrokes typed
// between the save and the broadcast are erased.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-edit-self-write-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".maestro", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);
const PROMPTS_DIR = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.prompts`);

// `prompt_file` follows the convention save_pipeline writes to (`<id>.prompts/<node>.md`)
// so the GET roundtrip can read back what the PUT wrote.
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
  - id: beta
    type: doc-only
    prompt_file: ${PIPELINE_NAME}.prompts/beta.md
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

test("post-save keystrokes survive the broadcast cycle (#17)", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  await page.locator('[title="Toggle edit mode"]').click();
  await page.getByRole("button", { name: new RegExp(PIPELINE_NAME) }).click();
  await page.getByText("alpha", { exact: true }).first().click();

  const promptArea = page.getByPlaceholder("Enter the node's role prompt...");
  await expect(promptArea).toBeVisible();

  // First edit + explicit save (auto-save removed in #35).
  await promptArea.fill("MARKER_A");
  await page.getByTestId("save-button").click();

  // Wait for the watcher broadcast cycle to settle.
  await page.waitForTimeout(1900);

  // Second edit, performed in the danger window between the daemon's write
  // and the (would-be) broadcast.
  await promptArea.fill("MARKER_A_then_MARKER_B");

  // Explicit save again, then cover the broadcast/reload window.
  await page.getByTestId("save-button").click();
  await page.waitForTimeout(3000);

  await expect(promptArea).toHaveValue("MARKER_A_then_MARKER_B");

  // Persistence: the explicit save flushed MARKER_B to disk.
  await page.reload();
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });
  await page.locator('[title="Toggle edit mode"]').click();
  await page.getByRole("button", { name: new RegExp(PIPELINE_NAME) }).click();
  await page.getByText("alpha", { exact: true }).first().click();

  const reloaded = page.getByPlaceholder("Enter the node's role prompt...");
  await expect(reloaded).toHaveValue("MARKER_A_then_MARKER_B");
});
