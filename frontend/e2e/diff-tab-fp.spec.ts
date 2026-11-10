import { test, expect } from "@playwright/test";
import { execFileSync } from "node:child_process";
import * as fs from "node:fs/promises";
import * as os from "node:os";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { cleanupRuns, runMultipart, E2E_TARGET_REPO } from "./helpers";

// FP validation — #748 (ADR-0067): the Run-level Diff tab.
//
//  1. A Run whose node changed two files of the target repo.
//  2. Open the Run and its Diff tab → both files appear with correct +/- counts
//     (compared against `git diff --numstat` on the run branch).
//  3. Collapse a file → only its header remains; expand it → its added and
//     deleted lines are visible and coloured.
//  4. Open an archived Run → "Diff not preserved".
//
// Real daemon (webServer), real run branch and worktree. The stub node session
// (`sleep`) never delivers, so the spec commits the two files on the Run's
// branch itself, through the daemon-created pipeline worktree — exactly what a
// node's delivery would have left there.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const SHOTS = process.env.PDO_FP_SHOTS_DIR ?? path.join(WORKSPACE_ROOT, "fp-shots");
const PIPELINE_NAME = `e2e-diff-tab-fp-${process.pid}-${Date.now()}`;
// Instance pipelines live under `$HOME/.pdo/pipelines` — the daemon's instance
// root — not under the workspace's `.pdo/` (the Run's blackboard).
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
  - id: worker
    name: worker
    type: agent
    isolated_worktree: false
    inputs:
      - { name: task, side: top }
    outputs:
      - { name: summary, side: bottom }
    view: { x: 100, y: 150 }
  - id: end
    name: End
    type: end
    inputs:
      - { name: result, side: top }
    view: { x: 100, y: 300 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: summary }
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

async function openInfoPanel(page: import("@playwright/test").Page, runId: string) {
  await page
    .getByText(runId.slice(0, 20))
    .first()
    .click({ timeout: 5_000, position: { x: 5, y: 5 } });
  await page.waitForTimeout(500);
  await page.getByTestId("toolbar-info").click();
  await expect(page.getByTestId("pipeline-info-panel")).toBeVisible({ timeout: 3_000 });
}

test("Diff tab lists the Run's changed files with correct counts, collapses and expands", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  const resp = await page.request.post(`${baseURL}/runs`, {
    multipart: runMultipart({ pipeline: PIPELINE_NAME, input: "diff tab fp" }),
  });
  expect(resp.status()).toBe(201);
  const { run_id } = await resp.json();
  createdRuns.push(run_id);

  // The daemon cuts `pdo/run-<id>` and its worktree at creation.
  const wt = path.join(E2E_TARGET_REPO, ".pdo", "runs", run_id, "worktree");
  await expect
    .poll(async () => fs.stat(wt).then(() => true, () => false), { timeout: 10_000 })
    .toBe(true);

  // Step 1 — two TRACKED files modified on the Run's branch, so each carries
  // both additions and deletions (a new file would be a pure addition).
  const readme = path.join(wt, "README.md");
  const gitignore = path.join(wt, ".gitignore");
  const readmeLines = (await fs.readFile(readme, "utf8")).split("\n");
  readmeLines[0] = "# e2e diff tab — edited title";
  readmeLines.splice(1, 0, "", "Added by the #748 FP.");
  await fs.writeFile(readme, readmeLines.join("\n"));
  const giLines = (await fs.readFile(gitignore, "utf8")).split("\n");
  giLines[0] = "# e2e diff tab — edited comment";
  await fs.writeFile(gitignore, giLines.join("\n"));
  git(wt, ["add", "README.md", ".gitignore"]);
  git(wt, ["-c", "user.email=e2e@test", "-c", "user.name=e2e", "commit", "-q", "-m", "fp: edit"]);

  // Ground truth for the counters: git itself.
  const fork = git(E2E_TARGET_REPO, ["merge-base", "HEAD", `pdo/run-${run_id}`]);
  const numstat = git(E2E_TARGET_REPO, [
    "diff",
    "--numstat",
    `${fork}...pdo/run-${run_id}`,
    "--",
    ".",
    ":(exclude).pdo/",
  ])
    .split("\n")
    .filter(Boolean)
    .map((l) => l.split("\t"))
    .map(([a, d, p]) => ({ add: Number(a), del: Number(d), path: p }));
  expect(numstat.map((n) => n.path).sort()).toEqual([".gitignore", "README.md"]);
  for (const n of numstat) {
    expect(n.add).toBeGreaterThan(0);
    expect(n.del).toBeGreaterThan(0);
  }

  // Step 2 — the Diff tab shows both files with those counts.
  await openInfoPanel(page, run_id);
  await page.getByTestId("info-tab-diff").click();
  const tab = page.getByTestId("diff-tab");
  await expect(tab).toBeVisible({ timeout: 10_000 });
  await expect(page.getByTestId("diff-summary")).toContainText("2 files");

  const rows = page.getByTestId("diff-ledger-row");
  await expect(rows).toHaveCount(2);
  for (const n of numstat) {
    const row = rows.filter({ hasText: n.path });
    await expect(row).toContainText(`+${n.add}`);
    await expect(row).toContainText(`−${n.del}`);
  }
  const total = numstat.reduce((s, n) => ({ add: s.add + n.add, del: s.del + n.del }), { add: 0, del: 0 });
  await expect(page.getByTestId("diff-summary")).toContainText(`+${total.add}`);
  await expect(page.getByTestId("diff-summary")).toContainText(`−${total.del}`);
  await page.getByTestId("pipeline-info-panel").screenshot({
    path: path.join(SHOTS, "748-diff-tab-loaded.png"),
  });

  // Every file starts expanded; its coloured +/- rows match git's counts.
  const files = page.getByTestId("diff-file");
  await expect(files).toHaveCount(2);
  await expect(page.getByTestId("diff-file-body")).toHaveCount(2);
  const readmeStat = numstat.find((n) => n.path === "README.md")!;
  const alpha = page.locator("[data-testid='diff-file'][data-path='README.md']");
  await expect(alpha.getByTestId("diff-line-del")).toHaveCount(readmeStat.del);
  await expect(alpha.getByTestId("diff-line-add")).toHaveCount(readmeStat.add);
  await expect(alpha.getByTestId("diff-line-add").first()).toContainText("e2e diff tab — edited title");
  // Coloured: an added and a deleted row are painted, each its own tint.
  const tint = (kind: "add" | "del") =>
    alpha.getByTestId(`diff-line-${kind}`).first().evaluate((row) => getComputedStyle(row).backgroundColor);
  const [addTint, delTint] = [await tint("add"), await tint("del")];
  for (const c of [addTint, delTint]) expect(c).not.toMatch(/^rgba\(0, 0, 0, 0\)$|^transparent$/);
  expect(addTint).not.toBe(delTint);

  // Step 3 — collapse: header only; expand: lines back.
  await alpha.getByTestId("diff-file-header").click();
  await expect(alpha).toHaveAttribute("data-collapsed", "true");
  await expect(alpha.getByTestId("diff-file-body")).toHaveCount(0);
  await expect(alpha.getByTestId("diff-file-header")).toContainText("README.md");
  await alpha.getByTestId("diff-file-header").click();
  await expect(alpha).toHaveAttribute("data-collapsed", "false");
  await expect(alpha.getByTestId("diff-line-del")).toHaveCount(readmeStat.del);
  await expect(alpha.getByTestId("diff-line-add")).toHaveCount(readmeStat.add);

  // The collapse state survives Info ↔ Diff.
  await alpha.getByTestId("diff-file-header").click();
  await page.getByTestId("info-tab-info").click();
  await expect(page.getByTestId("run-stats")).toBeVisible();
  // …and Info's Changes stat is the way back in (no Diff section left in Info).
  await expect(page.getByTestId("diff-section")).toHaveCount(0);
  await page.getByTestId("stat-loc-open-diff").click();
  await expect(alpha).toHaveAttribute("data-collapsed", "true", { timeout: 10_000 });
});

test("an archived Run says the diff is not preserved", async ({ page, baseURL }) => {
  await page.goto("/");
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  const resp = await page.request.post(`${baseURL}/runs`, {
    multipart: runMultipart({ pipeline: PIPELINE_NAME, input: "diff tab archived" }),
  });
  expect(resp.status()).toBe(201);
  const { run_id } = await resp.json();
  createdRuns.push(run_id);

  // Step 4 — archive (cleanup deletes the run branch; the run stays listed
  // under "Archived" until forgotten in afterAll), then open the Diff tab.
  const archive = await page.request.post(`${baseURL}/runs/${run_id}/commands`, {
    data: { kind: "cleanup_run" },
  });
  expect(archive.ok()).toBe(true);
  await page.reload();
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });
  // Archived runs live in a collapsed "Archived" section — expand it if the
  // run row isn't already on screen.
  const archivedToggle = page.getByTestId("run-archived-chevron");
  await expect(archivedToggle).toBeVisible({ timeout: 8_000 });
  const archivedRow = page.getByText(run_id.slice(0, 20)).first();
  if (!(await archivedRow.isVisible().catch(() => false))) {
    await archivedToggle.click();
    await expect(archivedRow).toBeVisible({ timeout: 5_000 });
  }
  await openInfoPanel(page, run_id);
  await page.getByTestId("info-tab-diff").click();
  await expect(page.getByTestId("diff-archived")).toBeVisible({ timeout: 10_000 });
  await expect(page.getByTestId("diff-archived")).toHaveText("Diff not preserved");
  await expect(page.getByTestId("diff-archived")).toHaveAttribute("title", "The run branch was deleted at cleanup");
  await expect(page.getByText("No changes")).toHaveCount(0);
  await page.getByTestId("pipeline-info-panel").screenshot({
    path: path.join(SHOTS, "748-diff-tab-archived.png"),
  });
});

test("in a narrow pane, an added file's header stays inside it", async ({ page, baseURL }) => {
  await page.goto("/");
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  const resp = await page.request.post(`${baseURL}/runs`, {
    multipart: runMultipart({ pipeline: PIPELINE_NAME, input: "diff tab narrow" }),
  });
  expect(resp.status()).toBe(201);
  const { run_id } = await resp.json();
  createdRuns.push(run_id);
  const wt = path.join(E2E_TARGET_REPO, ".pdo", "runs", run_id, "worktree");
  await expect
    .poll(async () => fs.stat(wt).then(() => true, () => false), { timeout: 10_000 })
    .toBe(true);

  // A new file with a long name: the header carries the « new » chip on top of its counts.
  const name = "a_very_long_new_file_name_that_does_not_fit_a_narrow_pane_3.ts";
  await fs.mkdir(path.join(wt, "src", "deeply", "nested"), { recursive: true });
  await fs.writeFile(path.join(wt, "src", "deeply", "nested", name), "export const a = 1;\n");
  git(wt, ["add", "src"]);
  git(wt, ["-c", "user.email=e2e@test", "-c", "user.name=e2e", "commit", "-q", "-m", "fp: add"]);

  await openInfoPanel(page, run_id);
  await page.getByTestId("info-tab-diff").click();
  const tab = page.getByTestId("diff-tab");
  await expect(page.getByTestId("diff-file-header")).toHaveCount(1, { timeout: 10_000 });
  await expect(page.getByTestId("diff-file-badge")).toHaveText("new");

  // The pane at its narrowest drawn width.
  await tab.evaluate((el) => {
    el.style.width = "240px";
  });
  for (const testId of ["diff-file-header", "diff-ledger-row"]) {
    const overflow = await page.getByTestId(testId).evaluate((row) => {
      const right = row.getBoundingClientRect().right;
      return [...row.children].map((c) => c.getBoundingClientRect().right - right).filter((d) => d > 0.5);
    });
    expect(overflow, testId).toEqual([]);
  }
  // The name keeps its end in clear.
  await expect(page.getByTestId("diff-file-header")).toContainText("_pane_3.ts");
});
