import { test, expect } from "@playwright/test";
import type { Page } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { openRunNodeDetails, cleanupRuns } from "./helpers";

// Layer 3b — Inputs/Outputs sections + MarkdownArtifactModal (#27).
//
// Post-refonte: the IO sections live in the Run inspector's details pane. An
// output port that has artifacts on disk renders as a clickable `button.port-row`
// that opens the MarkdownArtifactModal; a port with no files renders as a
// non-interactive `div.port-row`. The reviewer runs (so the daemon tracks a
// NodeRun and the run pane shows its IO); we seed its `review` output and leave
// `notes` empty, exercising both the interactive and non-interactive cases.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-io-modal-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);

const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
    view: { x: 0, y: 0 }
  - id: reviewer
    name: reviewer
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
      - name: notes
    view: { x: 100, y: 120 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
    view: { x: 0, y: 260 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: reviewer, port: task }
  - source: { node: reviewer, port: review }
    target: { node: end, port: result }
`;

let runId: string;

test.beforeAll(async () => {
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await cleanupRuns(runId);
});

async function waitForReviewerRunning(page: Page, baseURL: string, rid: string) {
  await expect(async () => {
    const resp = await page.request.get(`${baseURL}/runs/${rid}`);
    expect(resp.status()).toBe(200);
    const json = await resp.json();
    expect(json.nodes?.reviewer?.status).toBe("running");
  }).toPass({ timeout: 10_000 });
}

async function createRunAndSeedArtifacts(page: Page, baseURL: string) {
  const resp = await page.request.post(`${baseURL}/runs`, {
    multipart: { pipeline: PIPELINE_NAME, input: "e2e IO modal test" },
  });
  expect(resp.status()).toBe(201);
  const json = await resp.json();
  runId = json.run_id;

  // Seed the `review` output (the `notes` output stays empty on purpose).
  // Output artifacts live at <artifacts>/<node>/iter-<N>/<port>/output.md.
  const reviewDir = path.join(
    WORKSPACE_ROOT, ".pdo", "runs", runId, "worktree", ".pdo", "artifacts",
    "reviewer", "iter-1", "review",
  );
  await fs.mkdir(reviewDir, { recursive: true });
  await fs.writeFile(
    path.join(reviewDir, "output.md"),
    "---\nverdict: PASS\n---\n\n## Review\n\nAll looks good. **No issues** found.",
  );

  await waitForReviewerRunning(page, baseURL, runId);
  return runId;
}

/** The seeded `review` output card (the one port-row that is a button). */
function reviewCard(page: Page) {
  return page
    .getByTestId("inspector-pane-run")
    .locator("button.port-row")
    .first();
}

test("clicking anywhere on port card opens modal with markdown + frontmatter", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  await createRunAndSeedArtifacts(page, baseURL!);
  await openRunNodeDetails(page, runId, "reviewer");

  const portCard = reviewCard(page);
  await expect(portCard).toBeVisible({ timeout: 5_000 });
  await portCard.click();

  const modal = page.locator(".artifact-markdown");
  await expect(modal).toBeVisible({ timeout: 3_000 });
  await expect(modal).toContainText("Review");
  await expect(modal).toContainText("No issues");

  // Frontmatter card with the typed verdict. It is a sibling of the markdown
  // body inside the modal overlay (not inside `.artifact-markdown`); scope to
  // the overlay so the output port card's own `verdict` chip doesn't collide.
  const overlay = page
    .locator(".fixed.inset-0.z-50")
    .filter({ has: page.locator(".artifact-markdown") });
  await expect(overlay.getByText("verdict")).toBeVisible();
  await expect(overlay.getByText("PASS")).toBeVisible();

  // Close via the X button (scope to the modal overlay so we don't grab an
  // inspector icon-button instead).
  const closeBtn = overlay.locator("button").filter({ has: page.locator("svg") }).last();
  await closeBtn.click();
  await expect(modal).not.toBeVisible({ timeout: 2_000 });
});

test("modal closes on Escape key", async ({ page, baseURL }) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  if (!runId) await createRunAndSeedArtifacts(page, baseURL!);
  await openRunNodeDetails(page, runId, "reviewer");

  const portCard = reviewCard(page);
  await expect(portCard).toBeVisible({ timeout: 5_000 });
  await portCard.click();

  const modal = page.locator(".artifact-markdown");
  await expect(modal).toBeVisible({ timeout: 3_000 });

  await page.keyboard.press("Escape");
  await expect(modal).not.toBeVisible({ timeout: 2_000 });
});

test("modal closes on backdrop click", async ({ page, baseURL }) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  if (!runId) await createRunAndSeedArtifacts(page, baseURL!);
  await openRunNodeDetails(page, runId, "reviewer");

  const portCard = reviewCard(page);
  await expect(portCard).toBeVisible({ timeout: 5_000 });
  await portCard.click();

  const modal = page.locator(".artifact-markdown");
  await expect(modal).toBeVisible({ timeout: 3_000 });

  // Click the backdrop (the fixed overlay behind the modal).
  await page.mouse.click(10, 10);
  await expect(modal).not.toBeVisible({ timeout: 2_000 });
});

test("port card with no files renders as non-interactive div", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  if (!runId) await createRunAndSeedArtifacts(page, baseURL!);
  await openRunNodeDetails(page, runId, "reviewer");

  // The `notes` output has no seeded files, so its port row is a div, not a button.
  const notesRow = page
    .getByTestId("inspector-pane-run")
    .locator(".port-row")
    .filter({ hasText: "notes" });
  await expect(notesRow).toBeVisible({ timeout: 5_000 });
  const tag = await notesRow.evaluate((el) => el.tagName.toLowerCase());
  expect(tag).toBe("div");
});

test("port card opens modal via keyboard (Enter key)", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  if (!runId) await createRunAndSeedArtifacts(page, baseURL!);
  await openRunNodeDetails(page, runId, "reviewer");

  const portCard = reviewCard(page);
  await expect(portCard).toBeVisible({ timeout: 5_000 });

  await portCard.focus();
  await page.keyboard.press("Enter");

  const modal = page.locator(".artifact-markdown");
  await expect(modal).toBeVisible({ timeout: 3_000 });

  await page.keyboard.press("Escape");
  await expect(modal).not.toBeVisible({ timeout: 2_000 });
});
