import { test, expect } from "@playwright/test";
import type { Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import * as fs from "node:fs/promises";
import * as os from "node:os";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { cleanupRuns, openRunNodeDetails, runMultipart, E2E_TARGET_REPO } from "./helpers";

// FP validation — #749 (ADR-0067): the Review page.
//
//  1. On a finished Run where two nodes delivered different files, open the
//     Diff tab then « Expand and comment » → the Review page opens, pair
//     fork → tip, every file listed on the left, side-by-side.
//  2. Pick the first node's `after` as destination and its `before` as source →
//     only that node's files remain listed (checked against git).
//  3. Expand the context between two hunks of a file → the hidden lines appear.
//  4. From the second node's panel, use the shortcut → the Review opens with
//     that node's delivery preselected.
//
// Real daemon (webServer), real Run branch, real deliveries: each node's work is
// written into the Run's shared worktree and its completion is signalled on
// `POST /runs/<id>/nodes/<node>/done` — exactly what `pdo complete` does — so
// the daemon itself commits the delivery and records `NodeDelivered`.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const SHOTS = process.env.PDO_FP_SHOTS_DIR ?? path.join(WORKSPACE_ROOT, "fp-shots");
const PIPELINE_NAME = `e2e-review-fp-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(os.homedir(), ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);

const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - { name: user_prompt, side: bottom }
    view: { x: 100, y: 0 }
  - id: alpha
    name: alpha
    type: agent
    isolated_worktree: false
    inputs:
      - { name: task, side: top }
    outputs:
      - { name: summary, side: bottom }
    view: { x: 100, y: 150 }
  - id: beta
    name: beta
    type: agent
    isolated_worktree: false
    inputs:
      - { name: task, side: top }
    outputs:
      - { name: summary, side: bottom }
    view: { x: 100, y: 300 }
  - id: end
    name: End
    type: end
    inputs:
      - { name: result, side: top }
    view: { x: 100, y: 450 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: alpha, port: task }
  - source: { node: alpha, port: summary }
    target: { node: beta, port: task }
  - source: { node: beta, port: summary }
    target: { node: end, port: result }
`;

function git(cwd: string, args: string[]): string {
  return execFileSync("git", args, { cwd, encoding: "utf8" }).trim();
}

const createdRuns: string[] = [];

test.beforeAll(async () => {
  await fs.mkdir(SHOTS, { recursive: true });
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async () => {
  await cleanupRuns(...createdRuns);
  await fs.rm(PIPELINE_PATH, { force: true });
});

interface RunView {
  status: string;
  nodes: Record<string, { status: string; delivery?: { before: string; after: string } | null }>;
}

async function waitForNode(page: Page, baseURL: string, runId: string, nodeId: string, pred: (n: RunView["nodes"][string]) => boolean) {
  await expect
    .poll(
      async () => {
        const r = await page.request.get(`${baseURL}/runs/${runId}`);
        if (!r.ok()) return false;
        const view = (await r.json()) as RunView;
        const n = view.nodes[nodeId];
        return !!n && pred(n);
      },
      { timeout: 30_000 },
    )
    .toBe(true);
}

/** A node's work: files written into the Run's worktree, then `done` (what `pdo complete` does). */
async function deliver(page: Page, baseURL: string, runId: string, wt: string, nodeId: string, write: () => Promise<void>) {
  await waitForNode(page, baseURL, runId, nodeId, (n) => n.status === "running" || n.status === "awaiting_user");
  await write();
  const artifact = path.join(wt, ".pdo", "artifacts", nodeId, "iter-1", "summary");
  await fs.mkdir(artifact, { recursive: true });
  await fs.writeFile(path.join(artifact, "output.md"), `# ${nodeId}\n\ndone\n`);
  const done = await page.request.post(`${baseURL}/runs/${runId}/nodes/${nodeId}/done`, {
    data: { iter: 1 },
  });
  expect(done.ok(), `node ${nodeId} done: ${done.status()} ${await done.text()}`).toBe(true);
  await waitForNode(page, baseURL, runId, nodeId, (n) => !!n.delivery);
}

async function openInfoPanel(page: Page, runId: string) {
  await page
    .getByText(runId.slice(0, 20))
    .first()
    .click({ timeout: 5_000, position: { x: 5, y: 5 } });
  await page.waitForTimeout(500);
  await page.getByTestId("toolbar-info").click();
  await expect(page.getByTestId("pipeline-info-panel")).toBeVisible({ timeout: 3_000 });
}

test("Review page: fork → tip, one node's delivery, context expansion, node shortcut", async ({ page, baseURL }) => {
  test.setTimeout(150_000);
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  const resp = await page.request.post(`${baseURL}/runs`, {
    multipart: runMultipart({ pipeline: PIPELINE_NAME, input: "review fp" }),
  });
  expect(resp.status()).toBe(201);
  const { run_id } = await resp.json();
  createdRuns.push(run_id);

  const wt = path.join(E2E_TARGET_REPO, ".pdo", "runs", run_id, "worktree");
  await expect
    .poll(async () => fs.stat(wt).then(() => true, () => false), { timeout: 10_000 })
    .toBe(true);

  // --- alpha delivers: README edited in TWO places (a gap of hidden lines
  // between the hunks) + a new file.
  const readmePath = path.join(wt, "README.md");
  const original = (await fs.readFile(readmePath, "utf8")).split("\n");
  expect(original.length).toBeGreaterThan(80);
  // The probe sits between the two hunks (lines 3 and 62), outside their context
  // lines, and stays untouched: it is hidden until the context is expanded. Picked
  // as the first distinctive line of that gap rather than a fixed index, since the
  // README's head keeps being rewritten.
  const HIDDEN_PROBE = original
    .slice(10, 55)
    .find((line) => line.trim().length > 20 && original.indexOf(line) === original.lastIndexOf(line)) ?? "";
  expect(HIDDEN_PROBE.length, "a distinctive README line between the hunks").toBeGreaterThan(20);
  await deliver(page, baseURL!, run_id, wt, "alpha", async () => {
    const lines = [...original];
    lines[2] = "alpha edited line 3 (#749 FP)";
    lines[61] = "alpha edited line 62 (#749 FP)";
    await fs.writeFile(readmePath, lines.join("\n"));
    await fs.writeFile(path.join(wt, "alpha-note.md"), "# alpha\n\nwritten by alpha\n");
  });

  // --- beta delivers: a different file.
  await deliver(page, baseURL!, run_id, wt, "beta", async () => {
    await fs.writeFile(path.join(wt, "beta-note.md"), "# beta\n\nwritten by beta\n");
  });

  // Ground truth from git, through the refs the daemon exposes.
  const refsResp = await page.request.get(`${baseURL}/runs/${run_id}/refs`);
  expect(refsResp.ok()).toBe(true);
  const refs = (await refsResp.json()) as {
    refs: { id: string; sha: string | null; label: string }[];
    deliveries: { node_id: string; iter: number; before: string; after?: string }[];
  };
  expect(refs.refs.map((r) => r.id)).toEqual([
    "fork",
    "node:alpha:1:before",
    "node:alpha:1:after",
    "node:beta:1:before",
    "node:beta:1:after",
    "tip",
    // #835: the Run's working tree is a ref while it exists, listed last.
    "worktree",
  ]);
  const sha = (id: string) => refs.refs.find((r) => r.id === id)!.sha!;
  const alphaFiles = git(E2E_TARGET_REPO, [
    "diff", "--name-only", sha("node:alpha:1:before"), sha("node:alpha:1:after"), "--", ".", ":(exclude).pdo/",
  ])
    .split("\n")
    .filter(Boolean)
    .sort();
  expect(alphaFiles).toEqual(["README.md", "alpha-note.md"]);
  const allFiles = git(E2E_TARGET_REPO, [
    "diff", "--name-only", `${sha("fork")}...pdo/run-${run_id}`, "--", ".", ":(exclude).pdo/",
  ])
    .split("\n")
    .filter(Boolean)
    .sort();
  expect(allFiles).toEqual(["README.md", "alpha-note.md", "beta-note.md"]);

  // ===== Step 1 — Diff tab → « Expand and comment » → Review page, fork → tip.
  await page.reload();
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });
  await openInfoPanel(page, run_id);
  await page.getByTestId("info-tab-diff").click();
  await expect(page.getByTestId("diff-tab")).toBeVisible({ timeout: 10_000 });
  const expand = page.getByTestId("diff-review-button");
  await expect(expand).toHaveText(/Expand and comment/);
  await expand.click();

  await expect(page).toHaveURL(new RegExp(`/runs/${run_id}/review$`));
  await expect(page.getByTestId("review-page")).toBeVisible({ timeout: 10_000 });
  await expect(page.getByTestId("review-from")).toHaveText(/Fork point/, { timeout: 10_000 });
  // #835: while the Run's worktree exists, the default destination is the
  // working tree (commits + uncommitted edits), no longer the Run tip.
  await expect(page.getByTestId("review-to")).toHaveText(/Working tree/);
  await expect(page.getByTestId("review-view-toggle")).toHaveAttribute("data-view", "split");
  const rows = page.getByTestId("review-file-row");
  await expect(rows).toHaveCount(3, { timeout: 10_000 });
  const listed = async () =>
    (await rows.evaluateAll((els) => els.map((e) => e.getAttribute("data-path")))).filter(Boolean).sort();
  expect(await listed()).toEqual(allFiles);
  await expect(page.getByTestId("review-stats")).toContainText("3 files");
  // Side-by-side: the third-party body renders both columns.
  const readmeCard = page.locator('[data-testid="review-file"][data-path="README.md"]');
  await expect(readmeCard.locator('tr[data-side="old"]').first()).toBeVisible({ timeout: 10_000 });
  await expect(readmeCard.locator('tr[data-side="new"]').first()).toBeVisible();
  await page.screenshot({ path: path.join(SHOTS, "749-review-fork-tip.png") });

  // The page is a real URL: reload keeps it.
  await page.reload();
  await expect(page.getByTestId("review-page")).toBeVisible({ timeout: 10_000 });
  await expect(rows).toHaveCount(3, { timeout: 10_000 });

  // ===== Step 2 — destination = alpha's after, source = alpha's before.
  await page.getByTestId("review-to").click();
  await page.getByTestId("review-ref-popover-to").getByTestId("review-ref-option-node:alpha:1:after").click();
  await expect(page.getByTestId("review-to")).toHaveText(/alpha · iter 1 · after/);
  await page.getByTestId("review-from").click();
  await page.getByTestId("review-ref-popover-from").getByTestId("review-ref-option-node:alpha:1:before").click();
  await expect(page.getByTestId("review-from")).toHaveText(/alpha · iter 1 · before/);
  await expect(page).toHaveURL(/from=node%3Aalpha%3A1%3Abefore&to=node%3Aalpha%3A1%3Aafter/);
  await expect(page.getByTestId("review-delivery-chip")).toContainText("Reviewing the delivery of alpha · iter 1");
  await expect(rows).toHaveCount(2, { timeout: 10_000 });
  expect(await listed()).toEqual(alphaFiles);
  await page.screenshot({ path: path.join(SHOTS, "749-review-alpha-delivery.png") });

  // ===== Step 3 — expand the context between README's two hunks.
  await expect(readmeCard).toHaveAttribute("data-content", "ready", { timeout: 10_000 });
  const probe = readmeCard.getByText(HIDDEN_PROBE, { exact: true });
  await expect(probe).toHaveCount(0);
  const hunkRowsBefore = await readmeCard.locator('tr[data-state="hunk"]').count();
  expect(hunkRowsBefore).toBeGreaterThanOrEqual(2);
  // The gap is ~58 lines: GitHub-style ↑/↓ (20 lines each) or ⇕ when small.
  const expander = readmeCard
    .locator('button[title="Expand All"], button[title="Expand Down"], button[title="Expand Up"]')
    .first();
  await expect(expander).toBeVisible({ timeout: 10_000 });
  let clicks = 0;
  while ((await probe.count()) === 0 && clicks < 6) {
    const btn = readmeCard
      .locator('button[title="Expand All"], button[title="Expand Down"], button[title="Expand Up"]')
      .first();
    await btn.click();
    clicks += 1;
    await page.waitForTimeout(150);
  }
  await expect(probe.first()).toBeVisible();
  await page.screenshot({ path: path.join(SHOTS, "749-review-context-expanded.png") });

  // ===== Step 4 — from beta's node panel, the shortcut preselects its delivery.
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });
  await openRunNodeDetails(page, run_id, "beta");
  const shortcut = page.getByTestId("node-review-shortcut");
  await expect(shortcut).toHaveText(/Review this node's delivery/);
  await shortcut.click();
  await expect(page).toHaveURL(/from=node%3Abeta%3A1%3Abefore&to=node%3Abeta%3A1%3Aafter/);
  await expect(page.getByTestId("review-delivery-chip")).toContainText("Reviewing the delivery of beta · iter 1", {
    timeout: 10_000,
  });
  await expect(rows).toHaveCount(1, { timeout: 10_000 });
  expect(await listed()).toEqual(["beta-note.md"]);
  await page.screenshot({ path: path.join(SHOTS, "749-review-beta-shortcut.png") });
});

test("the Split | Unified toggle is remembered and « Nothing to compare » for the same ref", async ({ page, baseURL }) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });
  const resp = await page.request.post(`${baseURL}/runs`, {
    multipart: runMultipart({ pipeline: PIPELINE_NAME, input: "review toggle" }),
  });
  expect(resp.status()).toBe(201);
  const { run_id } = await resp.json();
  createdRuns.push(run_id);

  await page.goto(`/runs/${run_id}/review`);
  await expect(page.getByTestId("review-page")).toBeVisible({ timeout: 10_000 });
  await expect(page.getByTestId("review-view-toggle")).toHaveAttribute("data-view", "split");
  await page.getByTestId("review-view-unified").click();
  await expect(page.getByTestId("review-view-toggle")).toHaveAttribute("data-view", "unified");
  await page.reload();
  await expect(page.getByTestId("review-view-toggle")).toHaveAttribute("data-view", "unified", { timeout: 10_000 });
  expect(await page.evaluate(() => localStorage.getItem("pdo.review.view"))).toBe("unified");

  await page.goto(`/runs/${run_id}/review?from=tip&to=tip`);
  await expect(page.getByTestId("review-empty")).toContainText("Nothing to compare", { timeout: 10_000 });
});
