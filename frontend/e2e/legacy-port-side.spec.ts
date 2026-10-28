import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as os from "node:os";
import * as path from "node:path";
import { openPipelineForEdit } from "./helpers";

// Layer 3b — a port has no side nor `repeated` any more.
//
// A pipeline saved before still declares them: `side: bottom` and
// `repeated: true` on an output, `side: top` on End's `result`. It reopens with
// its wire leaving from the middle of the bottom border (the daemon's parse
// turned the side into a `source_anchor` without offset), and the output's card
// in the inspector offers neither L/T/R/B nor rpt: its type sits in the name row.

const PIPELINE_NAME = `e2e-legacy-port-side-${process.pid}-${Date.now()}`;
// The instance registry (`~/.pdo/pipelines`), where the daemon lists pipelines.
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
      - { name: user_prompt, side: right }
    view: { x: 0, y: 100 }
  - id: writer
    name: writer
    type: agent
    isolated_worktree: false
    outputs:
      - { name: verdict, side: bottom, repeated: true }
    view: { x: 200, y: 100 }
  - id: end
    name: End
    type: end
    inputs:
      - { name: result, side: top }
    view: { x: 200, y: 320 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: writer, port: task }
  - source: { node: writer, port: verdict }
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

test("a legacy pipeline reopens without port side nor rpt, its wire leaving where it did", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });
  await openPipelineForEdit(page, PIPELINE_NAME);

  const writer = page.locator('.react-flow__node[data-id="writer"]');
  await expect(writer).toBeVisible({ timeout: 5_000 });

  // The wire writer -> End leaves from the middle of the writer's bottom border
  // and lands on End's top.
  const departure = await page.evaluate(() => {
    const edges = Array.from(document.querySelectorAll<SVGPathElement>(".react-flow__edge-path"));
    const out = edges
      .map((p) => {
        const start = p.getPointAtLength(0);
        const end = p.getPointAtLength(p.getTotalLength());
        const ctm = p.getScreenCTM()!;
        const toScreen = (pt: DOMPoint) => new DOMPoint(pt.x, pt.y).matrixTransform(ctm);
        return { start: toScreen(start), end: toScreen(end) };
      });
    return out.map(({ start, end }) => ({ sx: start.x, sy: start.y, ex: end.x, ey: end.y }));
  });
  const box = (await writer.boundingBox())!;
  const endBox = (await page.locator('.react-flow__node[data-id="end"]').boundingBox())!;
  const wire = departure.find(
    (d) => Math.abs(d.sy - (box.y + box.height)) <= 3 && Math.abs(d.sx - (box.x + box.width / 2)) <= 3,
  );
  expect(wire, JSON.stringify({ departure, box })).toBeTruthy();
  // It lands on the End's top border.
  expect(wire!.ey).toBeLessThanOrEqual(endBox.y + 3);

  // The output's card: no side picker, no rpt toggle; the type in the name row.
  await page.getByText("writer", { exact: true }).first().click();
  const card = page.getByTestId("output-port-card-verdict");
  await expect(card).toBeVisible();
  for (const side of ["left", "top", "right", "bottom"]) {
    await expect(card.getByTitle(side, { exact: true })).toHaveCount(0);
  }
  await expect(card.getByRole("switch", { name: /repeated/i })).toHaveCount(0);
  await expect(card.getByText("rpt", { exact: true })).toHaveCount(0);
  const nameRow = card.locator(".op-head .port-row");
  await expect(nameRow.getByTestId("port-type-select")).toHaveValue("markdown");
});
