import { test, expect } from "@playwright/test";
import { openPipelineForEdit } from "./helpers";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Layer 3b — Node icons E2E (refs #67).
// Post canvas-refonte (ADR-0011 / #146 / #151 / #171) the first-class node
// types are only start / end / merge — `switch`, `loop` and `for-each` were
// removed as node TYPES (a fan-out / cycle is now a loop *region*, not a node).
// The backend still parses the legacy variants but migrates them onto generic
// agent nodes, so they render with the agent icon and no code/doc marker.
//
// This spec seeds start + two generic agents (one doc-only, one code-mutating)
// + merge + end and asserts: structural icons for start/end/merge, the agent
// icon for the generics, no text pills, and exactly the two explicit code/doc
// markers.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-node-icons-${process.pid}-${Date.now()}`;
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
    view: { x: 0, y: 300 }
  - id: planner
    name: Planner
    type: doc-only
    inputs:
      - name: in
        side: left
    outputs:
      - name: plan
        side: right
    view: { x: 250, y: 200 }
  - id: implementer
    name: Implementer
    type: code-mutating
    inputs:
      - name: in
        side: left
    outputs:
      - name: out
        side: right
    view: { x: 250, y: 400 }
  - id: merger
    name: Merger
    type: merge
    inputs:
      - name: branches
        repeated: true
        side: left
    outputs:
      - name: merged
        side: right
    view: { x: 750, y: 400 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
        side: left
    outputs: []
    view: { x: 1000, y: 300 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: planner, port: in }
  - source: { node: start, port: user_prompt }
    target: { node: implementer, port: in }
  - source: { node: planner, port: plan }
    target: { node: merger, port: branches }
  - source: { node: implementer, port: out }
    target: { node: merger, port: branches }
  - source: { node: merger, port: merged }
    target: { node: end, port: result }
`;

test.beforeAll(async () => {
  await fs.mkdir(PROMPTS_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
  await fs.writeFile(path.join(PROMPTS_DIR, "planner.md"), "Plan the work.\n");
  await fs.writeFile(path.join(PROMPTS_DIR, "implementer.md"), "Implement the plan.\n");
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await fs.rm(PROMPTS_DIR, { recursive: true, force: true });
});

test("each node type renders its structural icon", async ({ page }) => {
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

  // First-class structural icons.
  await expect(page.locator("[data-testid='node-icon-start']").first()).toBeVisible({ timeout: 3_000 });
  await expect(page.locator("[data-testid='node-icon-end']").first()).toBeVisible({ timeout: 3_000 });
  await expect(page.locator("[data-testid='node-icon-merge']").first()).toBeVisible({ timeout: 3_000 });

  // Generic agent nodes (doc-only + code-mutating) render the agent icon.
  const agentIcons = page.locator("[data-testid='node-icon-agent']");
  await expect(agentIcons).toHaveCount(2, { timeout: 3_000 });

  expect(consoleErrors).toEqual([]);
});

test("no text pills are present on any node", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  await openPipelineForEdit(page, PIPELINE_NAME);
  await page.waitForTimeout(500);

  // Wait for the canvas to render (scope to the canvas node, not the Library
  // list entry which also surfaces the pipeline/node names).
  await expect(
    page.getByTestId("rf__node-planner").getByText("Planner"),
  ).toBeVisible({ timeout: 5_000 });

  // None of the old type-pill texts should appear as bordered badge elements.
  // We target the specific pattern: a small bordered span used as a pill label.
  // The pill pattern was: <span class="...rounded border...">text</span>
  // After removal, these words may still appear as node labels or IDs but
  // not as bordered pill badges.
  const pillTexts = ["doc", "code", "switch", "loop", "foreach", "merge"];
  for (const pillText of pillTexts) {
    // Count elements that exactly match the pill text — this catches pills
    // but not node labels like "Review Loop" which contain "loop" as substring.
    const exactPills = page.locator(
      `span.border:has-text("${pillText}"), span.border-acc:has-text("${pillText}")`
    );
    await expect(exactPills).toHaveCount(0);
  }
});

test("code-mutating and doc-only markers are present and visually distinct", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  await openPipelineForEdit(page, PIPELINE_NAME);
  await page.waitForTimeout(500);

  // There should be exactly 2 code/doc markers (one per generic agent node)
  const markers = page.locator("[data-testid='code-doc-marker']");
  await expect(markers).toHaveCount(2, { timeout: 3_000 });

  // One should be code-mutating, one should be doc-only
  const codeMarker = page.locator("[data-testid='code-doc-marker'][data-marker-type='code-mutating']");
  const docMarker = page.locator("[data-testid='code-doc-marker'][data-marker-type='doc-only']");
  await expect(codeMarker).toHaveCount(1);
  await expect(docMarker).toHaveCount(1);
});
