import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { expectNonZeroBBox } from "./assertions";

// Layer 3b — Start pseudo-node + StartInspector (#30).
// Verifies: selecting the Run start node shows the StartInspector with header,
// runtime badge, input text inline, and "View as markdown" link opening modal.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-start-node-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".maestro", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);
const PROMPTS_DIR = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.prompts`);

const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: only
    type: doc-only
    prompt_file: ${PIPELINE_NAME}.prompts/only.md
    inputs:
      - name: task
    outputs:
      - name: out
    view: { x: 200, y: 100 }
edges: []
`;

const ROLE_PROMPT = "You are a worker. Do the thing.\n";

let runId: string;

test.beforeAll(async () => {
  process.env.MAESTRO_TMUX_CMD_OVERRIDE = "exec sleep 300";
  await fs.mkdir(PROMPTS_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
  await fs.writeFile(path.join(PROMPTS_DIR, "only.md"), ROLE_PROMPT);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await fs.rm(PROMPTS_DIR, { recursive: true, force: true });
  delete process.env.MAESTRO_TMUX_CMD_OVERRIDE;
  if (runId) {
    const { execSync } = await import("node:child_process");
    try {
      execSync(`tmux kill-session -t maestro-${runId}-only-iter-1`, {
        stdio: "ignore",
      });
    } catch {
      // session may already be dead
    }
  }
});

async function createRun(baseURL: string): Promise<string> {
  const resp = await fetch(`${baseURL}/runs`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      pipeline: PIPELINE_NAME,
      input: "hello from start node test",
    }),
  });
  expect(resp.status).toBe(201);
  const json = await resp.json();
  runId = json.run_id;
  return runId;
}

test("clicking start node shows StartInspector with header and input text", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  const rid = await createRun(baseURL!);

  // Wait for the run to appear in the list and click it
  await page.getByText(rid.slice(0, 8)).first().click({ timeout: 5_000 });

  const reactFlow = page.locator(".react-flow");
  await expect(reactFlow).toBeVisible({ timeout: 5_000 });
  await expectNonZeroBBox(reactFlow);

  // Wait for the start node to appear and click it
  await page.waitForTimeout(500);
  const startNode = page.locator(".start-node").first();
  await expect(startNode).toBeVisible({ timeout: 3_000 });
  await expectNonZeroBBox(startNode);
  await startNode.click();

  // StartInspector should appear
  const inspector = page.locator(".start-inspector");
  await expect(inspector).toBeVisible({ timeout: 3_000 });

  // Header should show "Run start"
  await expect(inspector.getByText("Run start")).toBeVisible();

  // Should show "runtime" badge
  await expect(inspector.locator(".runtime-badge")).toContainText("runtime");

  // Should show subtitle
  await expect(inspector.getByText("pseudo-node")).toBeVisible();

  // Should show the input text inline
  const inputPre = inspector.locator(".start-input-text");
  await expect(inputPre).toContainText("hello from start node test", {
    timeout: 5_000,
  });
});

test("view as markdown link opens modal", async ({ page, baseURL }) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  if (!runId) {
    await createRun(baseURL!);
  }

  await page.getByText(runId.slice(0, 8)).first().click({ timeout: 5_000 });
  await page.waitForTimeout(500);
  await page.locator(".start-node").first().click();

  const inspector = page.locator(".start-inspector");
  await expect(inspector).toBeVisible({ timeout: 3_000 });

  // Click "View as markdown ↗"
  const viewLink = inspector.locator(".view-markdown-link");
  await expect(viewLink).toBeVisible({ timeout: 3_000 });
  await viewLink.click();

  // The MarkdownArtifactModal should open
  const modal = page.locator(".artifact-markdown");
  await expect(modal).toBeVisible({ timeout: 3_000 });

  // Modal should contain the input text
  await expect(modal).toContainText("hello from start node test");

  // Close via Escape
  await page.keyboard.press("Escape");
  await expect(modal).not.toBeVisible({ timeout: 2_000 });
});
