import { test, expect, type Page } from "@playwright/test";
import { confirmSaveVersion, openPipelineForEdit } from "./helpers";
import * as fs from "node:fs/promises";
import * as os from "node:os";
import * as path from "node:path";

// Layer 3b — the canvas at any zoom (CORE-88).
//
// The rim a wire is pulled from keeps 8px on screen at 25 %, 100 % and 200 %
// (at most 3px inside the card), a wire leaves from the very pixel pressed, a
// drawn route does not move when the canvas zooms, and renaming a wired output
// applies on validation only — its edges and the downstream inputs follow.

const PIPELINE_NAME = `e2e-rim-zoom-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(os.homedir(), ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);
const PROMPTS_DIR = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.prompts`);

const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
    view: { x: -300, y: 0 }
  - id: designer
    name: designer
    type: agent
    isolated_worktree: false
    outputs:
      - name: spec
      - name: verdict
    view: { x: 0, y: 0 }
  - id: builder
    name: builder
    type: agent
    isolated_worktree: false
    outputs:
      - name: out
    view: { x: 420, y: -120 }
  - id: reviewer
    name: reviewer
    type: agent
    isolated_worktree: false
    outputs:
      - name: out
    view: { x: 420, y: 160 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
    outputs: []
    view: { x: 800, y: 0 }
edges:
  - source: { node: designer, port: spec }
    target: { node: builder, port: spec }
  - source: { node: designer, port: spec }
    target: { node: reviewer, port: spec }
  - source: { node: start, port: user_prompt }
    target: { node: designer, port: user_prompt }
`;

test.beforeAll(async () => {
  await fs.mkdir(PROMPTS_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
  for (const id of ["designer", "builder", "reviewer"]) {
    await fs.writeFile(path.join(PROMPTS_DIR, `${id}.md`), `Do ${id}.\n`);
  }
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await fs.rm(PROMPTS_DIR, { recursive: true, force: true });
});

async function open(page: Page) {
  await page.goto("/");
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });
  await openPipelineForEdit(page, PIPELINE_NAME);
  await expect(page.locator('.react-flow__node[data-id="designer"]')).toBeVisible({ timeout: 5_000 });
}

async function zoom(page: Page): Promise<number> {
  return Number(await page.getByTestId("canvas-zoom").getAttribute("data-zoom"));
}

/**
 * Wheels over an empty spot of the pane until the zoom is `target`. The wheel
 * zooms by 2^(-deltaY × 0.002), so each turn asks for exactly the delta that
 * remains; the clamps (0.25 floor, 2 ceiling) make those two exact anyway.
 */
async function zoomTo(page: Page, target: number) {
  const pane = await page.locator(".react-flow__pane").boundingBox();
  if (!pane) throw new Error("no pane");
  await page.mouse.move(pane.x + 40, pane.y + pane.height - 60);
  for (let i = 0; i < 12; i++) {
    const current = await zoom(page);
    if (Math.abs(current - target) < 1e-3) break;
    await page.mouse.wheel(0, Math.log2(current / target) / 0.002);
    await page.waitForTimeout(150);
  }
  await expect.poll(() => zoom(page)).toBeCloseTo(target, 2);
}

/** The flow-space polyline of the edge `e-<i>`, read off its SVG path. */
async function edgePath(page: Page, i: number): Promise<string> {
  const d = await page
    .locator(`[data-testid="rf__edge-e-${i}"] path.react-flow__edge-path`)
    .getAttribute("d");
  if (!d) throw new Error(`edge e-${i} has no path`);
  return d;
}

test("the rim is 8px on screen at 25 %, 100 % and 200 %, at most 3px inside the card", async ({
  page,
}) => {
  await open(page);
  for (const target of [1, 0.25, 2]) {
    await zoomTo(page, target);
    const card = await page.locator('.react-flow__node[data-id="designer"]').boundingBox();
    const top = await page
      .locator('.react-flow__node[data-id="designer"] [data-testid="rim-source-top"]')
      .boundingBox();
    const left = await page
      .locator('.react-flow__node[data-id="designer"] [data-testid="rim-source-left"]')
      .boundingBox();
    if (!card || !top || !left) throw new Error("rim not rendered");
    expect(top.height).toBeCloseTo(8, 0);
    expect(left.width).toBeCloseTo(8, 0);
    const inside = top.y + top.height - card.y;
    expect(inside).toBeLessThanOrEqual(3.6);
    expect(inside).toBeLessThanOrEqual(card.height * 0.25 + 0.6);
  }
});

test("dezoomed, the card's middle still moves the card", async ({ page }) => {
  await open(page);
  await zoomTo(page, 0.25);
  const node = page.locator('.react-flow__node[data-id="builder"]');
  const before = await node.boundingBox();
  if (!before) throw new Error("card not rendered");
  const grab = { x: before.x + before.width / 2, y: before.y + before.height / 2 };
  await page.mouse.move(grab.x, grab.y);
  await page.mouse.down();
  await page.mouse.move(grab.x + 40, grab.y + 30, { steps: 6 });
  await page.mouse.up();
  await expect(page.getByTestId("drag-connection-line")).toHaveCount(0);
  const after = await node.boundingBox();
  if (!after) throw new Error("card gone");
  expect(after.x - before.x).toBeGreaterThan(20);
  await expect(page.locator('[data-testid^="rf__edge-"]')).toHaveCount(3);
});

test("dezoomed, a wire starts at the first try from the pixel pressed", async ({ page }) => {
  await open(page);
  await zoomTo(page, 0.25);
  const card = await page.locator('.react-flow__node[data-id="reviewer"]').boundingBox();
  const target = await page.locator('.react-flow__node[data-id="builder"]').boundingBox();
  if (!card || !target) throw new Error("cards not rendered");

  // Just outside the top border, off any grid line.
  const press = { x: card.x + card.width * 0.37, y: card.y - 2 };
  await page.mouse.move(press.x, press.y);
  await page.mouse.down();
  await page.mouse.move(press.x, press.y - 30, { steps: 4 });
  await expect(page.getByTestId("wiring-snap-badge")).toContainText("snap ×4 · 120px");
  await page.mouse.move(target.x + target.width / 2, target.y + target.height / 2, { steps: 8 });
  await page.mouse.up();

  await expect(page.locator('[data-testid="rf__edge-e-3"]')).toHaveCount(1);
  // The wire leaves from the pressed x, on the card's top border.
  const d = await edgePath(page, 3);
  const [x, y] = d.replace(/^M\s*/, "").split(/[\s,L]+/).map(Number);
  const viewport = await page.locator(".react-flow__viewport").evaluate((el) => {
    const m = new DOMMatrix(getComputedStyle(el).transform);
    return { tx: m.e, ty: m.f, k: m.a };
  });
  const pane = await page.locator(".react-flow").boundingBox();
  if (!pane) throw new Error("no pane");
  const pressFlowX = (press.x - pane.x - viewport.tx) / viewport.k;
  const cardTopFlow = (card.y - pane.y - viewport.ty) / viewport.k;
  expect(Math.abs(x - pressFlowX)).toBeLessThanOrEqual(1 / viewport.k + 1);
  expect(Math.abs(y - cardTopFlow)).toBeLessThanOrEqual(1 / viewport.k + 1);
});

test("zooming in and out does not move a drawn route", async ({ page }) => {
  await open(page);
  // The routes settle once every card is measured: read them when two reads agree.
  let previous = "";
  await expect
    .poll(async () => {
      const now = `${await edgePath(page, 0)}|${await edgePath(page, 1)}`;
      const stable = now === previous;
      previous = now;
      return stable;
    })
    .toBe(true);
  const before = [await edgePath(page, 0), await edgePath(page, 1)];
  await zoomTo(page, 0.25);
  expect([await edgePath(page, 0), await edgePath(page, 1)]).toEqual(before);
  await zoomTo(page, 2);
  expect([await edgePath(page, 0), await edgePath(page, 1)]).toEqual(before);
});

test("renaming a wired output applies on validation, its edges and inputs follow", async ({ page }) => {
  await open(page);
  await page.locator('.react-flow__node[data-id="designer"]').click();
  // Outside a Run, no « Run » tab: the edition is open at once.
  await expect(page.getByTestId("inspector-tab-run")).toHaveCount(0);

  const field = page.getByTestId("output-port-card-spec").getByLabel("Port name");
  await field.fill("spec-v2");
  // Nothing moves while typing.
  await expect(page.getByTestId("edge-output-label-e-0-spec")).toBeVisible();
  await expect(page.getByTestId("port-rename-pending")).toContainText("2 edges follow");
  // Under the card, as in the design, not in its head.
  await expect(page.getByTestId("output-port-notices-spec").getByTestId("port-rename-pending")).toBeVisible();
  await field.press("Enter");

  await expect(page.getByTestId("edge-output-label-e-0-spec-v2")).toBeVisible();
  await expect(page.getByTestId("edge-output-label-e-1-spec-v2")).toBeVisible();
  for (const id of ["builder", "reviewer"]) {
    await page.locator(`.react-flow__node[data-id="${id}"]`).click();
    await expect(page.getByTestId("inspector-pane-edit")).toContainText("spec-v2");
  }

  // A name the node already has is refused, the old one stays.
  await page.locator('.react-flow__node[data-id="designer"]').click();
  const again = page.getByTestId("output-port-card-spec-v2").getByLabel("Port name");
  await again.fill("verdict");
  await again.press("Enter");
  await expect(page.getByTestId("port-rename-refusal")).toContainText("\"verdict\" is taken");
  await again.press("Escape");
  await expect(page.getByTestId("output-port-card-spec-v2")).toBeVisible();
});

test("a broken edge is deleted from the canvas, and its lint goes with the save", async ({ page }) => {
  const name = `${PIPELINE_NAME}-broken`;
  const file = path.join(PIPELINE_DIR, `${name}.yaml`);
  const prompts = path.join(PIPELINE_DIR, `${name}.prompts`);
  await fs.mkdir(prompts, { recursive: true });
  await fs.writeFile(
    file,
    SEED_YAML.replace(`name: ${PIPELINE_NAME}`, `name: ${name}`) +
      `  - source: { node: reviewer, port: draft }\n    target: { node: end, port: result }\n`,
  );
  for (const id of ["designer", "builder", "reviewer"]) {
    await fs.writeFile(path.join(prompts, `${id}.md`), `Do ${id}.\n`);
  }
  try {
    await page.goto("/");
    await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });
    await openPipelineForEdit(page, name);
    const lint = page.getByTestId("lint-banner").getByText(/port 'draft'/);
    await expect(lint).toHaveCount(1);

    await expect(page.getByTestId("edge-broken-label-e-3")).toContainText("draft · port absent");
    await page.getByTestId("edge-broken-delete-e-3").click();
    await expect(page.getByTestId("edge-broken-label-e-3")).toHaveCount(0);
    await page.getByTestId("save-button").click();
    await confirmSaveVersion(page);

    await expect(lint).toHaveCount(0);
    expect(await fs.readFile(file, "utf8")).not.toContain("draft");
  } finally {
    await fs.rm(file, { force: true });
    await fs.rm(prompts, { recursive: true, force: true });
  }
});
