import { test, expect } from "@playwright/test";
import { openPipelineForEdit } from "./helpers";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Layer 3b — Collection region E2E (refs #60, ADR-0011).
//
// Post canvas-refonte (ADR-0011 / #146 / #151 / #171 / #269): `for-each` is no
// longer a node TYPE at all — `parse_pipeline` hard-refuses it. A fan-out is a
// top-level `loops:` region of `kind: collection` fanning `over` a `list`
// frontmatter field. A single-member collection region renders as a compact
// `⇉ …` badge on the member's card (data-testid `collection-badge`), the
// EditToolbar deliberately has NO ForEach add-button, and the region is
// created from the "Fan out over \"<field>\"" context-menu gesture on an
// eligible member (data-testid `ctx-fanout-<field>` — the gesture EXISTS now,
// #151 / #269 shipped it).

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".pdo", "pipelines");

const REGION_PIPELINE = `e2e-collection-region-${process.pid}-${Date.now()}`;
const GESTURE_PIPELINE = `e2e-collection-gesture-${process.pid}-${Date.now()}`;

// start → upstream (emits `items: list`) → worker → end. `withRegion` adds the
// collection region over [worker], fanning over `items`; without it the same
// pipeline is the eligible pre-gesture shape.
function seedYaml(name: string, withRegion: boolean): string {
  return `name: ${name}
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
  - id: worker
    name: worker
    type: doc-only
    inputs:
      - name: in
        side: left
    outputs:
      - name: out
        side: right
    view: { x: 500, y: 200 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
        side: left
    outputs: []
    view: { x: 750, y: 200 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: upstream, port: in }
  - source: { node: upstream, port: out }
    target: { node: worker, port: in }
  - source: { node: worker, port: out }
    target: { node: end, port: result }
${
  withRegion
    ? `loops:
  - id: per-item
    kind: collection
    over: items
    members: [worker]
`
    : ""
}`;
}

test.beforeAll(async () => {
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(
    path.join(PIPELINE_DIR, `${REGION_PIPELINE}.yaml`),
    seedYaml(REGION_PIPELINE, true),
  );
  await fs.writeFile(
    path.join(PIPELINE_DIR, `${GESTURE_PIPELINE}.yaml`),
    seedYaml(GESTURE_PIPELINE, false),
  );
});

test.afterAll(async () => {
  await fs.rm(path.join(PIPELINE_DIR, `${REGION_PIPELINE}.yaml`), { force: true });
  await fs.rm(path.join(PIPELINE_DIR, `${GESTURE_PIPELINE}.yaml`), { force: true });
});

test("single-member collection region renders the ⇉ badge on the member card", async ({
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

  await openPipelineForEdit(page, REGION_PIPELINE);
  await page.waitForTimeout(500);

  // The member renders with its label and the compact collection badge
  // (single-member region → badge, not a box; ADR-0011 / #148, #151).
  const workerCard = page.getByTestId("rf__node-worker");
  await expect(workerCard).toBeVisible({ timeout: 5_000 });
  await expect(workerCard.getByText("worker")).toBeVisible({ timeout: 3_000 });

  const badge = workerCard.getByTestId("collection-badge");
  await expect(badge).toHaveCount(1);
  await expect(badge).toContainText("⇉");

  expect(consoleErrors.filter((e) => !/Failed to load resource/.test(e))).toEqual([]);
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

  await openPipelineForEdit(page, REGION_PIPELINE);
  await page.waitForTimeout(500);

  // The edit toolbar is present, with add + merge buttons.
  await expect(page.getByTestId("edit-toolbar")).toBeVisible({ timeout: 3_000 });
  await expect(page.getByTestId("toolbar-add")).toBeVisible();
  await expect(page.getByTestId("toolbar-merge")).toBeVisible();

  // ...but NO ForEach add-button — a collection fan-out is created from the
  // context-menu gesture on members (#151 / #171 / #269), not by adding a node.
  await expect(page.getByTestId("toolbar-foreach")).toHaveCount(0);

  expect(consoleErrors.filter((e) => !/Failed to load resource/.test(e))).toEqual([]);
});

test("fan-out gesture on an eligible member creates the collection region", async ({
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

  // The variant WITHOUT a `loops:` region: worker receives `items: list` from
  // upstream and lives in no region, so it is fan-out eligible (#269).
  await openPipelineForEdit(page, GESTURE_PIPELINE);
  await page.waitForTimeout(500);

  const workerCard = page.getByTestId("rf__node-worker");
  await expect(workerCard).toBeVisible({ timeout: 5_000 });
  await expect(workerCard.getByTestId("collection-badge")).toHaveCount(0);

  // Right-click opens the node context menu with the fan-out entry.
  await workerCard.click({ button: "right" });
  const fanout = page.getByTestId("ctx-fanout-items");
  await expect(fanout).toBeVisible({ timeout: 3_000 });
  await expect(fanout).toContainText('Fan out over "items"');

  // Clicking it creates the single-member collection region → badge appears.
  await fanout.click();
  const badge = workerCard.getByTestId("collection-badge");
  await expect(badge).toHaveCount(1, { timeout: 3_000 });
  await expect(badge).toContainText("⇉");

  expect(consoleErrors.filter((e) => !/Failed to load resource/.test(e))).toEqual([]);
});
