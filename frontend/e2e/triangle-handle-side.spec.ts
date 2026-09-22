import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { runMultipart } from "./helpers";

// Layer 4 — a declared output `side` places NO canvas handle any more (#40, #844).
//
// History: the output port used to be an SVG triangle, then a plain filled dot
// (#149 / #170) whose xyflow Handle carried `data-handlepos="<side>"`, and this
// spec asserted the dot landed on the declared `side: top`.
//
// Since #844 there is no per-output handle at all: a wire starts from any point
// of the card's BORDER, through four rim source strips, and the side it leaves by
// is the side the author pressed — recorded per edge as `source_anchor`, not
// declared per port. `side` survives in the document (it is semantic, and the
// node library treats it as identity) but places nothing on the canvas. So the
// spec now asserts exactly that: no `result` handle anywhere, and the four rim
// strips instead.
//
// The daemon refuses to load a pipeline without exactly one start + one end node
// (crates/pdo-daemon/src/pipeline.rs), so the seed wraps the checker between a
// start and an end. `POST /runs` is multipart/form-data post-refonte.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-triangle-side-${process.pid}-${Date.now()}`;
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
    view: { x: 0, y: 100 }
  - id: checker
    name: checker
    type: agent
    isolated_worktree: false
    prompt_file: ${PIPELINE_NAME}.prompts/checker.md
    inputs:
      - name: task
        side: left
    outputs:
      - name: result
        side: top
    view: { x: 200, y: 100 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
        side: left
    outputs: []
    view: { x: 400, y: 100 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: checker, port: task }
  - source: { node: checker, port: result }
    target: { node: end, port: result }
`;

let runId: string;

test.beforeAll(async () => {
  process.env.PDO_TMUX_CMD_OVERRIDE =
    "exec sh -c \"sleep 300\"";
  await fs.mkdir(PROMPTS_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
  await fs.writeFile(path.join(PROMPTS_DIR, "checker.md"), "Do the task.\n");
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await fs.rm(PROMPTS_DIR, { recursive: true, force: true });
  delete process.env.PDO_TMUX_CMD_OVERRIDE;
  if (runId) {
    const { execSync } = await import("node:child_process");
    try {
      execSync(`tmux kill-session -t pdo-${runId}-checker-iter-1`, {
        stdio: "ignore",
      });
    } catch {
      // session may already be dead
    }
  }
});

test("output port with side:top renders its dot handle on the top edge", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  const resp = await page.request.post(`${baseURL}/runs`, {
    multipart: runMultipart({
      pipeline: PIPELINE_NAME,
      input: "triangle side test",
    }),
  });
  expect(resp.status()).toBe(201);
  const json = await resp.json();
  runId = json.run_id;

  await page.getByText(runId.slice(0, 20)).first().click({ timeout: 5_000, position: { x: 5, y: 5 } });

  const reactFlow = page.locator(".react-flow");
  await expect(reactFlow).toBeVisible({ timeout: 5_000 });

  // Wait for the node to render
  await page.waitForTimeout(500);

  // The checker declares `side: top` on its `result` output. That places nothing:
  // there is no per-output handle on a card any more (#844).
  await expect(
    page.locator('.react-flow__handle[data-handleid="result"][data-handlepos="top"]'),
  ).toHaveCount(0);
  await expect(page.locator(".port-dot")).toHaveCount(0);

  // What the card offers instead is its whole border, on all four sides — the
  // departure side is the one the author presses, not one the port declares.
  for (const side of ["top", "bottom", "left", "right"]) {
    await expect(
      page.locator(`.react-flow__handle[data-handleid="rim-${side}"]`).first(),
    ).toBeVisible({ timeout: 5_000 });
  }
});
