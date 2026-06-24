import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { openPipelineForEdit } from "./helpers";

// Layer 3b — canvas undo/redo (ADR-0014 / #226), the durable CI gate.
// Deterministic (no drag, which is flaky in Playwright — node-move undo lives in
// the Layer-5 scenario instead): delete the middle edge via its context menu,
// then undo (Ctrl+Z) restores it, redo (Ctrl+Y / Ctrl+Shift+Z) re-deletes it,
// and the toolbar buttons reflect the stack (disabled at the bottom/top).
//
// The nodes are laid out in a straight horizontal row so the alpha→beta edge is
// a clean segment whose hit area sits in the gap between the two nodes — making
// the right-click target deterministic.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-undo-redo-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);
const PROMPTS_DIR = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.prompts`);

// edges: start→alpha (e-0), alpha→beta (e-1, the deletion target), beta→end (e-2).
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
    type: doc-only
    inputs:
      - { name: in, side: left }
    outputs:
      - { name: out, side: right }
    view: { x: 250, y: 100 }
  - id: beta
    name: beta
    type: doc-only
    inputs:
      - { name: in, side: left }
    outputs:
      - { name: out, side: right }
    view: { x: 520, y: 100 }
  - id: end
    name: End
    type: end
    inputs:
      - { name: result, side: left }
    view: { x: 790, y: 100 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: alpha, port: in }
  - source: { node: alpha, port: out }
    target: { node: beta, port: in }
  - source: { node: beta, port: out }
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

test("delete edge → Ctrl+Z restores → Ctrl+Y re-deletes; toolbar reflects the stack", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  await openPipelineForEdit(page, PIPELINE_NAME);

  const alpha = page.locator('.react-flow__node[data-id="alpha"]');
  await expect(alpha).toBeVisible({ timeout: 5_000 });

  const edges = page.locator(".react-flow__edge");
  await expect(edges).toHaveCount(3);

  const undoBtn = page.getByTestId("toolbar-undo");
  const redoBtn = page.getByTestId("toolbar-redo");
  const tabTitle = page.getByTestId(`tab-title-${PIPELINE_NAME}`);

  // Fresh pipeline → empty history → both buttons disabled.
  await expect(undoBtn).toBeDisabled();
  await expect(redoBtn).toBeDisabled();
  await expect(tabTitle).toHaveText(`${PIPELINE_NAME}.yaml`);

  // The daemon's file watcher emits one delayed `pipeline_changed` for the seed
  // file we created in beforeAll. If it lands AFTER the destructive edit (while
  // the tab is dirty) it raises a conflict modal that swallows clicks. Wait it
  // out while the tab is still clean — a clean reload just no-ops here.
  await page.waitForTimeout(3500);
  await expect(undoBtn).toBeDisabled();

  // Right-click the alpha→beta edge (e-1) and choose "Delete edge". The edge is
  // a straight horizontal segment, so its <g> bbox centre lies on the path.
  // Playwright reports an SVG <g> as "hidden", so we force the right-click rather
  // than gate on visibility (the element resolves fine and is present in the DOM).
  const targetEdge = page.locator('.react-flow__edge[data-id="e-1"]');
  await expect(targetEdge).toHaveCount(1);
  await targetEdge.click({ button: "right", force: true });
  const deleteItem = page.getByText("Delete edge", { exact: true });
  await expect(deleteItem).toBeVisible();
  await deleteItem.click();

  // Edge gone, tab dirty, undo now possible, redo still not.
  await expect(edges).toHaveCount(2);
  await expect(tabTitle).toHaveText(`• ${PIPELINE_NAME}.yaml`);
  await expect(undoBtn).toBeEnabled();
  await expect(redoBtn).toBeDisabled();

  // Focus is on <body> after the context-menu item is removed (not a text
  // field), so the window-level shortcut fires. deleteEdge already reset the
  // selection, so no explicit deselect is needed.

  // Ctrl+Z restores the edge.
  await page.keyboard.press("Control+z");
  await expect(edges).toHaveCount(3);
  await expect(redoBtn).toBeEnabled();

  // Ctrl+Y re-applies the delete.
  await page.keyboard.press("Control+y");
  await expect(edges).toHaveCount(2);

  // Ctrl+Shift+Z is the other redo binding — first undo, then redo with it.
  await page.keyboard.press("Control+z");
  await expect(edges).toHaveCount(3);
  await page.keyboard.press("Control+Shift+z");
  await expect(edges).toHaveCount(2);

  // Undo back to baseline via the toolbar button, then assert it disables at the
  // bottom of the stack (no further undo possible).
  await undoBtn.click();
  await expect(edges).toHaveCount(3);
  await expect(undoBtn).toBeDisabled();

  // A redo is still available (the re-deleted state); the redo button reflects it.
  await expect(redoBtn).toBeEnabled();
});
