import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Layer 4 — Triangle handle renders on correct side based on port side field (#40).
// Creates a pipeline with side: top on the output port, launches a run,
// and asserts the handle SVG polygon renders on the top edge of the node.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-triangle-side-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);
const PROMPTS_DIR = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.prompts`);

const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: checker
    name: checker
    type: doc-only
    inputs:
      - name: task
        side: left
    outputs:
      - name: result
        side: top
    view: { x: 200, y: 100 }
edges: []
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

test("port with side:top renders handle on top edge with outward triangle", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  const resp = await page.request.post(`${baseURL}/runs`, {
    data: {
      pipeline: PIPELINE_NAME,
      input: "triangle side test",
    },
  });
  expect(resp.status()).toBe(201);
  const json = await resp.json();
  runId = json.run_id;

  await page.getByText(runId.slice(0, 8)).first().click({ timeout: 5_000 });

  const reactFlow = page.locator(".react-flow");
  await expect(reactFlow).toBeVisible({ timeout: 5_000 });

  // Wait for the node to render
  await page.waitForTimeout(500);

  // The output handle with side:top should be rendered as an xyflow Handle
  // with data-handlepos="top". xyflow sets this attribute based on Position.
  const topHandle = page.locator(
    '.react-flow__handle[data-handlepos="top"]',
  );
  await expect(topHandle).toBeVisible({ timeout: 5_000 });

  // The triangle SVG inside should have the outward-pointing polygon for
  // (output, top): "5,10 11,10 8,2" — pointing upward away from the node.
  const polygon = topHandle.locator("polygon");
  await expect(polygon).toBeVisible();
  const points = await polygon.getAttribute("points");
  expect(points).toBe("5,10 11,10 8,2");

  // The input handle should be on the left side
  const leftHandle = page.locator(
    '.react-flow__handle[data-handlepos="left"]',
  );
  await expect(leftHandle).toBeVisible({ timeout: 3_000 });

  // Input left triangle points inward (rightward): "2,5 2,11 10,8"
  const leftPolygon = leftHandle.locator("polygon");
  const leftPoints = await leftPolygon.getAttribute("points");
  expect(leftPoints).toBe("2,5 2,11 10,8");
});
