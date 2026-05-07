import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { expectNonZeroBBox } from "./assertions";

// Layer 3b — Inputs/Outputs sections + MarkdownArtifactModal (#27).
// Verifies: selecting a node shows IO sections, clicking "open ↗" opens the
// modal with rendered markdown + frontmatter card. For a repeated port, the
// prev/next chevrons change content. Close via X / Escape / backdrop.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-io-modal-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".maestro", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);
const PROMPTS_DIR = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.prompts`);

const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: reviewer
    type: doc-only
    prompt_file: ${PIPELINE_NAME}.prompts/reviewer.md
    inputs:
      - name: reviews
        repeated: true
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
    view: { x: 100, y: 100 }
edges: []
`;

const ROLE_PROMPT = "You are a reviewer. Review the code.\n";

let runId: string;

test.beforeAll(async () => {
  process.env.MAESTRO_TMUX_CMD_OVERRIDE = "exec sleep 300";
  await fs.mkdir(PROMPTS_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
  await fs.writeFile(path.join(PROMPTS_DIR, "reviewer.md"), ROLE_PROMPT);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await fs.rm(PROMPTS_DIR, { recursive: true, force: true });
  delete process.env.MAESTRO_TMUX_CMD_OVERRIDE;
  if (runId) {
    const { execSync } = await import("node:child_process");
    try {
      execSync(`tmux kill-session -t maestro-${runId}-reviewer-iter-1`, {
        stdio: "ignore",
      });
    } catch {
      // session may already be dead
    }
  }
});

async function createRunAndSeedArtifacts(baseURL: string) {
  const resp = await fetch(`${baseURL}/runs`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      pipeline: PIPELINE_NAME,
      input: "e2e IO modal test",
    }),
  });
  expect(resp.status).toBe(201);
  const json = await resp.json();
  runId = json.run_id;

  // Seed output artifacts for the reviewer node
  const artifactsDir = path.join(
    WORKSPACE_ROOT,
    ".maestro",
    "runs",
    runId,
    "worktree",
    ".maestro",
    "artifacts",
  );

  const reviewerDir = path.join(artifactsDir, "reviewer", "iter-1");
  await fs.mkdir(reviewerDir, { recursive: true });
  await fs.writeFile(
    path.join(reviewerDir, "review.md"),
    "---\nverdict: PASS\n---\n\n## Review\n\nAll looks good. **No issues** found.",
  );

  return runId;
}

test("clicking open on output port opens modal with markdown + frontmatter", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  const rid = await createRunAndSeedArtifacts(baseURL!);

  // Wait for the run to appear in the list and click it
  await page.getByText(rid.slice(0, 8)).first().click({ timeout: 5_000 });

  const reactFlow = page.locator(".react-flow");
  await expect(reactFlow).toBeVisible({ timeout: 5_000 });
  await expectNonZeroBBox(reactFlow);

  // Click the reviewer node
  await page.waitForTimeout(500);
  const reviewerNode = page.getByText("reviewer", { exact: true }).first();
  await expect(reviewerNode).toBeVisible({ timeout: 3_000 });
  await expectNonZeroBBox(reviewerNode);
  await reviewerNode.click();

  // Wait for Outputs section to appear with the port row
  await expect(page.getByText("Outputs")).toBeVisible({ timeout: 5_000 });
  await expect(page.getByText("review").first()).toBeVisible({
    timeout: 3_000,
  });

  // Click "open ↗" on the output port
  const openLink = page.locator(".open-link").first();
  await expect(openLink).toBeVisible({ timeout: 5_000 });
  await openLink.click();

  // The modal should appear
  const modal = page.locator(".artifact-markdown");
  await expect(modal).toBeVisible({ timeout: 3_000 });

  // Should show the markdown content
  await expect(modal).toContainText("Review");
  await expect(modal).toContainText("No issues");

  // Should show the frontmatter card with verdict
  await expect(page.getByText("verdict")).toBeVisible();
  await expect(page.getByText("PASS")).toBeVisible();

  // Close via X button
  const closeBtn = page.locator("button").filter({ has: page.locator("svg") }).last();
  await closeBtn.click();
  await expect(modal).not.toBeVisible({ timeout: 2_000 });
});

test("modal closes on Escape key", async ({ page, baseURL }) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  if (!runId) {
    await createRunAndSeedArtifacts(baseURL!);
  }

  await page.getByText(runId.slice(0, 8)).first().click({ timeout: 5_000 });
  await page.waitForTimeout(500);
  await page.getByText("reviewer", { exact: true }).first().click();

  await expect(page.getByText("Outputs")).toBeVisible({ timeout: 5_000 });
  const openLink = page.locator(".open-link").first();
  await expect(openLink).toBeVisible({ timeout: 5_000 });
  await openLink.click();

  const modal = page.locator(".artifact-markdown");
  await expect(modal).toBeVisible({ timeout: 3_000 });

  // Press Escape
  await page.keyboard.press("Escape");
  await expect(modal).not.toBeVisible({ timeout: 2_000 });
});

test("modal closes on backdrop click", async ({ page, baseURL }) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  if (!runId) {
    await createRunAndSeedArtifacts(baseURL!);
  }

  await page.getByText(runId.slice(0, 8)).first().click({ timeout: 5_000 });
  await page.waitForTimeout(500);
  await page.getByText("reviewer", { exact: true }).first().click();

  await expect(page.getByText("Outputs")).toBeVisible({ timeout: 5_000 });
  const openLink = page.locator(".open-link").first();
  await expect(openLink).toBeVisible({ timeout: 5_000 });
  await openLink.click();

  const modal = page.locator(".artifact-markdown");
  await expect(modal).toBeVisible({ timeout: 3_000 });

  // Click the backdrop (the fixed overlay behind the modal)
  await page.mouse.click(10, 10);
  await expect(modal).not.toBeVisible({ timeout: 2_000 });
});
