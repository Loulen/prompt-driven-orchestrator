import { test, expect } from "@playwright/test";
import { openPipelineForEdit } from "./helpers";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Layer 3b — ForEach node E2E (refs #60).
//
// Post canvas-refonte (ADR-0011 / #146 / #151 / #171): `for-each` is no longer a
// first-class node TYPE with its own icon or toolbar add-button — a fan-out is a
// `collection` loop *region*, not a node. A pipeline that still declares a
// legacy `for-each` node loads and renders it verbatim as a generic agent node
// (the `node-icon-agent` User glyph, emergent input), and the EditToolbar
// deliberately has NO ForEach add-button. This spec asserts that current
// behaviour: the legacy for-each node renders with its label + agent icon, and
// the toolbar carries no foreach button (only add + merge).

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-foreach-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);
const PROMPTS_DIR = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.prompts`);

const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    inputs: []
    outputs:
      - name: user_prompt
    view: { x: 0, y: 200 }
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
          items:
            type: list
    view: { x: 250, y: 200 }
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
    view: { x: 500, y: 200 }
  - id: worker
    name: worker
    type: doc-only
    inputs:
      - name: in
        side: left
    outputs:
      - name: out
        side: right
    view: { x: 750, y: 200 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
        side: left
    outputs: []
    view: { x: 1000, y: 200 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: upstream, port: in }
  - source: { node: upstream, port: out }
    target: { node: fe1, port: in }
  - source: { node: fe1, port: body }
    target: { node: worker, port: in }
  - source: { node: worker, port: out }
    target: { node: fe1, port: done }
  - source: { node: fe1, port: done }
    target: { node: end, port: result }
`;

test.beforeAll(async () => {
  await fs.mkdir(PROMPTS_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
  await fs.writeFile(path.join(PROMPTS_DIR, "upstream.md"), "Produce items.\n");
  await fs.writeFile(path.join(PROMPTS_DIR, "worker.md"), "Process one item.\n");
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await fs.rm(PROMPTS_DIR, { recursive: true, force: true });
});

test("legacy foreach node renders as a generic agent node", async ({
  page,
}) => {
  const consoleErrors: string[] = [];
  page.on("console", (msg) => {
    if (msg.type() === "error") consoleErrors.push(msg.text());
  });

  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  await openPipelineForEdit(page, PIPELINE_NAME);
  await page.waitForTimeout(500);

  // The legacy for-each node renders verbatim with its label.
  const feCard = page.getByTestId("rf__node-fe1");
  await expect(feCard).toBeVisible({ timeout: 5_000 });
  await expect(feCard.getByText("per-item")).toBeVisible({ timeout: 3_000 });

  // It carries the generic agent icon (no first-class foreach icon exists).
  await expect(feCard.locator("[data-testid='node-icon-agent']")).toHaveCount(1);
  await expect(page.locator("[data-testid='node-icon-foreach']")).toHaveCount(0);

  expect(consoleErrors).toEqual([]);
});

test("toolbar has no ForEach add-button (fan-out is a region, not a node)", async ({
  page,
}) => {
  const consoleErrors: string[] = [];
  page.on("console", (msg) => {
    if (msg.type() === "error") consoleErrors.push(msg.text());
  });

  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  await openPipelineForEdit(page, PIPELINE_NAME);
  await page.waitForTimeout(500);

  // The edit toolbar is present, with add + merge buttons.
  await expect(page.getByTestId("edit-toolbar")).toBeVisible({ timeout: 3_000 });
  await expect(page.getByTestId("toolbar-add")).toBeVisible();
  await expect(page.getByTestId("toolbar-merge")).toBeVisible();

  // ...but NO ForEach add-button — a collection fan-out is created from the
  // gesture on members, not by adding a node (#151 / #171).
  await expect(page.getByTestId("toolbar-foreach")).toHaveCount(0);

  expect(consoleErrors).toEqual([]);
});
