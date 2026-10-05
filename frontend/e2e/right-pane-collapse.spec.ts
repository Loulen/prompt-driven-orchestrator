import { test, expect } from "@playwright/test";
import type { Page } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { cleanupRuns, openPipelineForEdit, runMultipart } from "./helpers";

// Layer 3b — the right pane collapses when it has nothing to show (Notion #6 /
// #949, CONTEXT.md « Panneau de droite replié »). Verifies:
// 1. Home screen and a freshly opened template: the pane is collapsed, the
//    canvas takes the full width, the handle is hidden.
// 2. A node selection expands it; a click on the empty canvas collapses it.
// 3. It reopens at the width the user last dragged it to — also after a reload
//    (the collapse never overwrites the persisted width).
// 4. Opening Pipeline info with nothing selected expands it; closing collapses.
// 5. A Run tab with an empty selection keeps its Run panel on the right.
// 6. Unfolding re-frames the canvas: the clicked card is never left under the
//    pane (FP iteration 1), and a graph the user panned to the left is not
//    pushed out past the canvas's left edge (FP iteration 2).

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-right-pane-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);

const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    inputs: []
    outputs:
      - name: user_prompt
    view: { x: 0, y: 100 }
  - id: worker
    name: worker
    type: agent
    isolated_worktree: false
    inputs:
      - name: in
    outputs:
      - name: out
    view: { x: 200, y: 100 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
    outputs: []
    view: { x: 400, y: 100 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: in }
  - source: { node: worker, port: out }
    target: { node: end, port: result }
`;

const RIGHT = '[data-panel]#right';

async function rightWidth(page: Page): Promise<number> {
  return page.locator(RIGHT).evaluate((el) => el.getBoundingClientRect().width);
}

async function expectCollapsed(page: Page): Promise<void> {
  await expect.poll(() => rightWidth(page)).toBeLessThan(2);
  await expect(page.getByTestId("right-pane-handle")).toBeHidden();
}

async function expectExpandedAt(page: Page, width: number): Promise<void> {
  await expect.poll(async () => Math.abs((await rightWidth(page)) - width)).toBeLessThan(3);
  await expect(page.getByTestId("right-pane-handle")).toBeVisible();
}

async function selectWorker(page: Page): Promise<void> {
  await page.getByText("worker", { exact: true }).first().click();
}

/** The card lies fully inside the visible canvas, not under the pane. */
async function expectCardInFrame(page: Page, id: string): Promise<void> {
  const canvas = await page.locator(".react-flow").first().boundingBox();
  expect(canvas).toBeTruthy();
  await expect
    .poll(async () => {
      const card = await page.locator(`.react-flow__node[data-id="${id}"]`).boundingBox();
      if (!card) return false;
      return card.x >= canvas!.x - 1 && card.x + card.width <= canvas!.x + canvas!.width + 1;
    })
    .toBe(true);
}

async function clickEmptyCanvas(page: Page): Promise<void> {
  // A corner of the React Flow pane, well away from the three seeded nodes.
  const pane = page.locator(".react-flow__pane");
  const box = await pane.boundingBox();
  expect(box).toBeTruthy();
  await pane.click({ position: { x: box!.width - 40, y: box!.height - 40 } });
}

async function storedRightPct(page: Page): Promise<number | undefined> {
  return page.evaluate(() => {
    const raw = localStorage.getItem("pdo.layout.run");
    return raw ? (JSON.parse(raw) as Record<string, number>).right : undefined;
  });
}

async function gotoConnected(page: Page): Promise<void> {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });
}

test.beforeAll(async () => {
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
});

test.beforeEach(async ({ page }) => {
  // The FP's screen size: wide enough that the full-width fit view spreads the
  // graph past what the unfolded canvas shows.
  await page.setViewportSize({ width: 1600, height: 900 });
  await page.goto("/");
  await page.evaluate(() => localStorage.removeItem("pdo.layout.run"));
});

test("collapses when empty and reopens at the last chosen width, across a reload", async ({
  page,
}) => {
  await gotoConnected(page);
  // Home screen: nothing to show.
  await expectCollapsed(page);

  // A freshly opened template, nothing selected: still collapsed, and the
  // canvas takes the width the pane would have had.
  await openPipelineForEdit(page, PIPELINE_NAME);
  await expect(page.getByTestId("tab-list")).toBeVisible({ timeout: 10_000 });
  await expectCollapsed(page);

  // Selecting a node brings the pane back at its default width (25%).
  const groupWidth = await page
    .locator("[data-group]")
    .first()
    .evaluate((el) => el.getBoundingClientRect().width);
  await selectWorker(page);
  await expectExpandedAt(page, groupWidth * 0.25);
  // The graph was fit-viewed on the full-width canvas: unfolding keeps it
  // centred, so its far end does not slide under the pane (FP iteration 1).
  await expectCardInFrame(page, "worker");
  await expectCardInFrame(page, "end");
  const initial = await rightWidth(page);

  // Widen the pane by dragging its handle 80px to the left.
  const handle = page.getByTestId("right-pane-handle");
  const box = await handle.boundingBox();
  expect(box).toBeTruthy();
  await page.mouse.move(box!.x + box!.width / 2, box!.y + box!.height / 2);
  await page.mouse.down();
  await page.mouse.move(box!.x + box!.width / 2 - 80, box!.y + box!.height / 2, { steps: 5 });
  await page.mouse.up();
  await expect.poll(() => rightWidth(page)).toBeGreaterThan(initial + 60);
  const widened = await rightWidth(page);
  await expect.poll(() => storedRightPct(page)).toBeGreaterThan(25);
  const storedPct = await storedRightPct(page);

  // Empty canvas click: collapsed, and the persisted width is untouched.
  await clickEmptyCanvas(page);
  await expectCollapsed(page);
  expect(await storedRightPct(page)).toBeCloseTo(storedPct!, 1);

  // Back on a node: the pane reopens at the width the user chose, and the
  // clicked card stays in frame.
  await selectWorker(page);
  await expectExpandedAt(page, widened);
  await expectCardInFrame(page, "worker");
  await expectCardInFrame(page, "end");

  // The width survives a reload, collapse included.
  await clickEmptyCanvas(page);
  await expectCollapsed(page);
  await page.reload();
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });
  await openPipelineForEdit(page, PIPELINE_NAME);
  await expect(page.getByTestId("tab-list")).toBeVisible({ timeout: 10_000 });
  await clickEmptyCanvas(page);
  await expectCollapsed(page);
  await selectWorker(page);
  await expectExpandedAt(page, widened);
});

test("Pipeline info expands the pane; closing it with nothing selected collapses it", async ({
  page,
}) => {
  await gotoConnected(page);
  await openPipelineForEdit(page, PIPELINE_NAME);
  await expect(page.getByTestId("tab-list")).toBeVisible({ timeout: 10_000 });
  await clickEmptyCanvas(page);
  await expectCollapsed(page);

  await page.getByTestId("toolbar-info").click();
  const infoPanel = page.getByTestId("pipeline-info-panel");
  await expect(infoPanel).toBeVisible({ timeout: 3_000 });
  await expect.poll(() => rightWidth(page)).toBeGreaterThan(100);

  await page.getByTestId("info-panel-close").click();
  await expect(infoPanel).not.toBeVisible();
  await expectCollapsed(page);

  // `i` again toggles it open, and `i` once more closes it back to collapsed.
  await page.getByTestId("toolbar-info").click();
  await expect(infoPanel).toBeVisible({ timeout: 3_000 });
  await page.getByTestId("toolbar-info").click();
  await expect(infoPanel).not.toBeVisible();
  await expectCollapsed(page);
});

test("unfolding does not push a graph panned to the left out of the canvas", async ({ page }) => {
  await gotoConnected(page);
  await openPipelineForEdit(page, PIPELINE_NAME);
  await expect(page.getByTestId("tab-list")).toBeVisible({ timeout: 10_000 });
  await clickEmptyCanvas(page);
  await expectCollapsed(page);

  // Pan the fit-viewed graph 300px to the left: every card stays visible, but
  // the graph now sits in the left half of the canvas.
  const pane = page.locator(".react-flow__pane");
  const box = await pane.boundingBox();
  expect(box).toBeTruthy();
  const y = box!.y + box!.height - 60;
  await page.mouse.move(box!.x + box!.width - 60, y);
  await page.mouse.down();
  await page.mouse.move(box!.x + box!.width - 360, y, { steps: 10 });
  await page.mouse.up();
  await expectCardInFrame(page, "start");
  await expectCardInFrame(page, "end");

  // Unfolding shifts the view by at most what keeps Start inside the canvas.
  await page.getByTestId("toolbar-info").click();
  await expect(page.getByTestId("pipeline-info-panel")).toBeVisible({ timeout: 3_000 });
  await expect.poll(() => rightWidth(page)).toBeGreaterThan(100);
  await expectCardInFrame(page, "start");
  await expectCardInFrame(page, "end");

  // Same when End is the card that unfolds the pane.
  await page.getByTestId("info-panel-close").click();
  await expectCollapsed(page);
  await page.locator('.react-flow__node[data-id="end"]').click();
  await expect.poll(() => rightWidth(page)).toBeGreaterThan(100);
  await expectCardInFrame(page, "start");
  await expectCardInFrame(page, "end");
});

test("a Run tab with an empty selection keeps its Run panel", async ({
  page,
  baseURL,
}) => {
  await gotoConnected(page);
  const resp = await page.request.post(`${baseURL}/runs`, {
    multipart: runMultipart({ pipeline: PIPELINE_NAME, input: "e2e right pane" }),
  });
  expect(resp.status()).toBe(201);
  const { run_id } = await resp.json();

  try {
    // A live Run snaps its selection back to the running node, so an empty
    // selection only holds on a terminal one: fail the worker first.
    await expect
      .poll(async () => {
        const r = await page.request.get(`${baseURL}/runs/${run_id}`);
        return (await r.json()).nodes?.worker?.status;
      }, { timeout: 15_000 })
      .toBe("running");
    const fail = await page.request.post(`${baseURL}/runs/${run_id}/nodes/worker/fail`, {
      data: { reason: "e2e right pane", iter: 1 },
    });
    expect(fail.status()).toBe(200);
    await expect
      .poll(async () => {
        const r = await page.request.get(`${baseURL}/runs/${run_id}`);
        return (await r.json()).nodes?.worker?.status;
      }, { timeout: 15_000 })
      .toBe("failed");

    await page
      .getByText(run_id.slice(0, 20))
      .first()
      .click({ timeout: 5_000, position: { x: 5, y: 5 } });
    await expect(page.getByTestId("tab-list")).toBeVisible({ timeout: 10_000 });
    await clickEmptyCanvas(page);
    await expect(page.getByTestId("pipeline-info-panel")).toBeVisible({ timeout: 5_000 });
    await expect.poll(() => rightWidth(page)).toBeGreaterThan(100);
    await expect(page.getByTestId("right-pane-handle")).toBeVisible();
  } finally {
    await cleanupRuns(run_id);
  }
});
