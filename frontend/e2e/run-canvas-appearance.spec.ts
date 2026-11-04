import { test, expect, type Locator, type Page } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as os from "node:os";
import * as path from "node:path";
import { runBody } from "./helpers";

// Layer 3b — a Run's canvas and Info tab with Settings › Appearance, in a real
// browser: by default a card shows Elapsed time and Cost on one row (49 px) and
// the rows below rise by as much, wires straight; the editor keeps the detailed
// cards; « Compact nodes » puts every card on one line and greys the card's
// switches; the Info tab leaves out the fields and the sections hidden.

const PIPELINE_DIR = path.join(os.homedir(), ".pdo", "pipelines");
const PIPELINE = `e2e-canvas-appearance-${process.pid}-${Date.now()}`;

// A fork: plan, then build and docs side by side, joined by a manual edge into ship.
const SEED_YAML = `name: ${PIPELINE}
version: "1.0"
prompt_required: false
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
    view: { x: 230, y: 0 }
  - id: plan
    name: plan
    type: agent
    isolated_worktree: false
    outputs:
      - name: out
    view: { x: 150, y: 100 }
  - id: build
    name: build
    type: agent
    isolated_worktree: false
    outputs:
      - name: out
    view: { x: 0, y: 240 }
  - id: docs
    name: docs
    type: agent
    isolated_worktree: false
    outputs:
      - name: out
    view: { x: 300, y: 240 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
    view: { x: 230, y: 400 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: plan, port: in }
  - source: { node: plan, port: out }
    target: { node: build, port: in }
  - source: { node: plan, port: out }
    target: { node: docs, port: in }
  - source: { node: build, port: out }
    target: { node: docs, port: in }
    mode: manual
    waypoints:
      - { x: 254, y: 273 }
`;

test.beforeAll(async () => {
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(path.join(PIPELINE_DIR, `${PIPELINE}.yaml`), SEED_YAML);
});

test.afterAll(async () => {
  await fs.rm(path.join(PIPELINE_DIR, `${PIPELINE}.yaml`), { force: true });
  await fs.rm(path.join(PIPELINE_DIR, `${PIPELINE}.prompts`), { recursive: true, force: true });
});

/**
 * The Actor's Appearance, served to this page only, so the shared e2e daemon keeps
 * none: a change made here is kept by the stub alone.
 */
async function stubAppearance(page: Page) {
  let stored: unknown = null;
  await page.route("**/actors/me/appearance", async (route) => {
    if (route.request().method() === "PUT") stored = route.request().postDataJSON();
    await route.fulfill({ json: { appearance: stored } });
  });
}

async function box(locator: Locator) {
  const b = await locator.boundingBox();
  if (!b) throw new Error("no box");
  return b;
}

const card = (page: Page, id: string) => page.getByTestId(`rf__node-${id}`).getByTestId("node-card");

test("a Run's cards follow Settings › Appearance: shorter by default, one line with Compact nodes; the Info tab leaves hidden sections out", async ({
  page,
  baseURL,
}) => {
  const resp = await page.request.post(`${baseURL}/runs`, {
    data: runBody({ pipeline: PIPELINE, input: "seed", name: "canvas appearance", auto_name: false }),
  });
  expect(resp.status()).toBe(201);
  const { run_id: runId } = (await resp.json()) as { run_id: string };

  await page.setViewportSize({ width: 1440, height: 900 });
  await stubAppearance(page);
  await page.goto("/");
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });
  const row = page.locator(`[data-run-row="${runId}"]`);
  await expect(row).toBeVisible({ timeout: 10_000 });
  await row.click();

  // Default: Elapsed time and Cost on one row; no model, no iteration.
  await expect(card(page, "plan")).toBeVisible({ timeout: 10_000 });
  await expect(card(page, "plan").locator(".cv-n__cell")).toHaveCount(2);
  const zoom = (await box(card(page, "plan"))).width / 208;
  expect((await box(card(page, "plan"))).height / zoom).toBeCloseTo(49, 0);
  // The fork's row rose by the 16 px the cards lost; side by side, it stays level.
  const plan = await box(card(page, "plan"));
  const build = await box(card(page, "build"));
  const docs = await box(card(page, "docs"));
  expect((build.y - plan.y) / zoom).toBeCloseTo(140 - 16, 0);
  expect(docs.y).toBeCloseTo(build.y, 0);

  // Settings › Appearance › Run canvas: Compact nodes greys the card's switches.
  await page.getByTestId("open-settings").click();
  await page.getByTestId("settings-category-appearance").click();
  const block = page.getByTestId("appearance-block-run-canvas");
  await expect(block.getByRole("switch", { name: "Elapsed time" })).toBeEnabled();
  await block.getByRole("switch", { name: "Compact nodes" }).click();
  for (const name of ["Model", "Iteration", "Elapsed time", "Cost", "Isolation flag", "Loop badge", "Agent profile marker"]) {
    await expect(block.getByRole("switch", { name })).toBeDisabled();
  }
  await expect(block).toContainText("Compact nodes is on: cards show their icon, name and status only.");
  await expect(page.getByTestId("appearance-preview-run-canvas").locator("[data-compact]").first()).toBeVisible();

  // Run info tab: hide the whole Stats section.
  const info = page.getByTestId("appearance-block-run-info");
  for (const name of ["Duration", "Changes", "Est. cost"]) await info.getByRole("switch", { name }).click();
  await expect(page.getByTestId("appearance-preview-run-info").getByTestId("run-stats")).toHaveCount(0);
  await page.keyboard.press("Escape");

  // The canvas: every card on one line.
  await expect(card(page, "plan")).toHaveAttribute("data-compact", "true");
  expect((await box(card(page, "plan"))).height / zoom).toBeCloseTo(30, 0);
  await expect(card(page, "plan").locator(".cv-n__grid")).toHaveCount(0);

  // The Info tab: no Stats section, the Run section and the name as before.
  await page.getByTestId("toolbar-info").click();
  await expect(page.getByTestId("pipeline-info-panel")).toBeVisible({ timeout: 3_000 });
  await expect(page.getByTestId("info-panel-name")).toBeVisible();
  await expect(page.getByTestId("run-stats")).toHaveCount(0);
  await expect(page.getByTestId("run-status")).toHaveCount(0);

  await page.request.post(`${baseURL}/runs/${runId}/commands`, { data: { kind: "cleanup_run" } });
});
