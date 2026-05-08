import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Layer 5 — review-loop E2E (refs #52).
// Seeds a pipeline with Loop + Switch + Implementer + Reviewer,
// verifies edit-mode rendering and iter badge, then creates a run and
// drives two iterations via mark_node_done, asserting iter badge updates.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-loop-review-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".maestro", "pipelines");
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
  - id: loop1
    name: review-loop
    type: loop
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
    max_iter: 5
    view: { x: 250, y: 200 }
  - id: impl1
    name: implementer
    type: doc-only
    prompt_file: ${PIPELINE_NAME}.prompts/impl1.md
    inputs:
      - name: in
        side: left
    outputs:
      - name: out
        side: right
    view: { x: 500, y: 150 }
  - id: reviewer
    name: reviewer
    type: doc-only
    prompt_file: ${PIPELINE_NAME}.prompts/reviewer.md
    inputs:
      - name: in
        side: left
    outputs:
      - name: review
        side: right
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
    view: { x: 750, y: 150 }
  - id: sw1
    name: quality-gate
    type: switch
    inputs:
      - name: in
        side: left
    outputs:
      - name: pass
        side: right
        when:
          verdict: { eq: PASS }
      - name: default
        side: bottom
    view: { x: 1000, y: 150 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
        side: left
    outputs: []
    view: { x: 1250, y: 200 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: loop1, port: in }
  - source: { node: loop1, port: body }
    target: { node: impl1, port: in }
  - source: { node: impl1, port: out }
    target: { node: reviewer, port: in }
  - source: { node: reviewer, port: review }
    target: { node: sw1, port: in }
  - source: { node: sw1, port: pass }
    target: { node: loop1, port: break }
  - source: { node: loop1, port: done }
    target: { node: end, port: result }
`;

let runId: string;

test.beforeAll(async () => {
  process.env.MAESTRO_TMUX_CMD_OVERRIDE =
    "exec sh -c \"sleep 300\"";
  await fs.mkdir(PROMPTS_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
  await fs.writeFile(path.join(PROMPTS_DIR, "impl1.md"), "Implement the task.\n");
  await fs.writeFile(path.join(PROMPTS_DIR, "reviewer.md"), "Review the code.\n");
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await fs.rm(PROMPTS_DIR, { recursive: true, force: true });
  delete process.env.MAESTRO_TMUX_CMD_OVERRIDE;
  if (runId) {
    const { execSync } = await import("node:child_process");
    const sessions = [
      `maestro-${runId}-impl1-iter-1`,
      `maestro-${runId}-reviewer-iter-1`,
      `maestro-${runId}-sw1-iter-1`,
      `maestro-mgr-${runId}`,
    ];
    for (const s of sessions) {
      try {
        execSync(`tmux kill-session -t ${s}`, { stdio: "ignore" });
      } catch {
        // session may already be dead
      }
    }
  }
});

test("loop node renders in edit mode with correct iter badge", async ({
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

  // Switch to edit mode
  const editToggle = page.locator("[data-testid='edit-toggle']");
  await expect(editToggle).toBeVisible({ timeout: 3_000 });
  await editToggle.click();

  // Select the pipeline from the list
  await page.getByText(PIPELINE_NAME).first().click({ timeout: 5_000 });
  await page.waitForTimeout(500);

  // Verify the loop node renders with iter badge "max 5"
  const loopNode = page.getByText("review-loop").first();
  await expect(loopNode).toBeVisible({ timeout: 5_000 });

  const iterBadge = page.locator("[data-testid='iter-badge']").first();
  await expect(iterBadge).toBeVisible({ timeout: 3_000 });
  await expect(iterBadge).toContainText("max 5");

  // Verify the switch node renders
  await expect(page.getByText("quality-gate").first()).toBeVisible({
    timeout: 3_000,
  });

  expect(consoleErrors).toEqual([]);
});

test("loop run mode: create run and verify loop node renders with iter badge", async ({
  page,
  baseURL,
}) => {
  const consoleErrors: string[] = [];
  page.on("console", (msg) => {
    if (msg.type() === "error") consoleErrors.push(msg.text());
  });

  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  // Create a run via the API
  const resp = await page.request.post(`${baseURL}/runs`, {
    data: {
      pipeline: PIPELINE_NAME,
      input: "loop review E2E test",
    },
  });
  expect(resp.status()).toBe(201);
  const json = await resp.json();
  runId = json.run_id;

  // Click the run from the list
  await page
    .getByText(runId.slice(0, 8))
    .first()
    .click({ timeout: 5_000 });

  // Wait for the canvas to render
  const reactFlow = page.locator(".react-flow");
  await expect(reactFlow).toBeVisible({ timeout: 5_000 });

  // The loop node should appear with the "loop" badge
  await expect(page.getByText("review-loop").first()).toBeVisible({
    timeout: 5_000,
  });

  // Wait for impl1 to start (the loop fires body → impl1)
  await page.waitForTimeout(2_000);

  // The loop iter badge should show the current iteration
  const iterBadge = page.locator("[data-testid='iter-badge']").first();
  await expect(iterBadge).toBeVisible({ timeout: 5_000 });

  // In run mode, the badge should show k/N format (e.g. "1/5")
  await expect(iterBadge).toContainText(/\d+\/5/);

  // Seed impl1 artifacts and complete it
  const artifactsBase = path.join(
    WORKSPACE_ROOT,
    ".maestro",
    "runs",
    runId,
    "worktree",
    ".maestro",
    "artifacts",
  );
  const impl1Dir = path.join(artifactsBase, "impl1", "iter-1");
  await fs.mkdir(impl1Dir, { recursive: true });
  await fs.writeFile(
    path.join(impl1Dir, "out.md"),
    "---\n---\n\nImplementation done.\n",
  );

  const markDone = async (nodeId: string, iter: number) => {
    const r = await page.request.post(`${baseURL}/runs/${runId}/commands`, {
      data: { kind: "mark_node_done", node_id: nodeId, iter },
    });
    expect(r.status()).toBe(200);
  };

  await markDone("impl1", 1);
  await page.waitForTimeout(1_000);

  // Reviewer should now be running — seed its artifact with FAIL verdict
  const reviewerDir = path.join(artifactsBase, "reviewer", "iter-1");
  await fs.mkdir(reviewerDir, { recursive: true });
  await fs.writeFile(
    path.join(reviewerDir, "review.md"),
    "---\nverdict: FAIL\n---\n\nNeeds work.\n",
  );

  await markDone("reviewer", 1);
  await page.waitForTimeout(1_000);

  // Switch should now be running — seed dummy artifacts for its output ports
  const swDir = path.join(artifactsBase, "sw1", "iter-1");
  await fs.mkdir(swDir, { recursive: true });
  await fs.writeFile(path.join(swDir, "pass.md"), "---\n---\n");
  await fs.writeFile(path.join(swDir, "default.md"), "---\n---\n");

  await markDone("sw1", 1);

  // Wait for the loop to advance to iteration 2
  await page.waitForTimeout(2_000);

  // Check that the iter badge updated (should now show 2/5)
  await expect(iterBadge).toContainText("2/5", { timeout: 5_000 });

  expect(consoleErrors).toEqual([]);
});
