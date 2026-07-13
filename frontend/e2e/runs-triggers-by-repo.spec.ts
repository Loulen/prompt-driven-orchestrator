import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as os from "node:os";
import * as path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

// Layer 4 (e2e) per ADR 0004 — proves #258 end-to-end in a browser: the Runs and
// Triggers lists group by project (target repo), conditionally (only when ≥ 2
// distinct repos are present), with a null `target_repo` resolved to the daemon's
// own repo_root (no "Unassigned" bucket) and the raw `target_repo` left untouched.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-by-repo-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);
const PROMPTS_DIR = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.prompts`);

// `prompt_required: false` so a cron-only trigger with no input template / guard
// is accepted at creation.
const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
prompt_required: false
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
    view: { x: 0, y: 100 }
  - id: worker
    name: Worker
    type: doc-only
    prompt_file: ${PIPELINE_NAME}.prompts/worker.md
    inputs:
      - name: task
    outputs:
      - name: result
    view: { x: 200, y: 100 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
    view: { x: 400, y: 100 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
`;

let repoA = "";
let repoB = "";

function gitInit(dir: string) {
  execFileSync("git", ["init", "-b", "main"], { cwd: dir });
  execFileSync("git", ["config", "user.email", "t@t.c"], { cwd: dir });
  execFileSync("git", ["config", "user.name", "t"], { cwd: dir });
  execFileSync("git", ["commit", "--allow-empty", "-m", "init"], { cwd: dir });
}

test.beforeAll(async () => {
  await fs.mkdir(PROMPTS_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
  await fs.writeFile(path.join(PROMPTS_DIR, "worker.md"), "You are a worker.\n");
  repoA = await fs.mkdtemp(path.join(os.tmpdir(), "pdo258-alpha-"));
  repoB = await fs.mkdtemp(path.join(os.tmpdir(), "pdo258-beta-"));
  gitInit(repoA);
  gitInit(repoB);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await fs.rm(PROMPTS_DIR, { recursive: true, force: true });
  if (repoA) await fs.rm(repoA, { recursive: true, force: true });
  if (repoB) await fs.rm(repoB, { recursive: true, force: true });
});

// Wipe this spec's triggers before each test so the daemon state is
// deterministic regardless of `reuseExistingServer` or a prior local run.
// Scoped to `e2e-by-repo-` pipelines (not a global wipe): other spec files run
// in parallel workers against the same daemon, and deleting THEIR triggers
// mid-test breaks them (e.g. runs-filter's trigger-name resolution, #336).
test.beforeEach(async ({ page, baseURL }) => {
  const resp = await page.request.get(`${baseURL}/triggers`);
  const triggers = (await resp.json()) as Array<{ id: string; pipeline_id: string }>;
  for (const t of triggers) {
    if (t.pipeline_id.startsWith("e2e-by-repo-")) {
      await page.request.delete(`${baseURL}/triggers/${t.id}`);
    }
  }
});

async function createTrigger(
  page: import("@playwright/test").Page,
  baseURL: string,
  name: string,
  targetRepo: string | null,
) {
  const data: Record<string, unknown> = {
    name,
    pipeline_id: PIPELINE_NAME,
    cron: "0 0 1 1 *",
  };
  if (targetRepo) data.target_repo = targetRepo;
  const resp = await page.request.post(`${baseURL}/triggers`, { data });
  expect(resp.status(), `POST /triggers ${name}`).toBe(201);
}

async function openTriggersTab(page: import("@playwright/test").Page) {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });
  await page.getByRole("tab", { name: "Triggers" }).click();
}

test("Triggers list stays flat (no group header) when all triggers share one repo", async ({
  page,
  baseURL,
}) => {
  await createTrigger(page, baseURL!, "flat-1", repoA);
  await createTrigger(page, baseURL!, "flat-2", repoA);

  await openTriggersTab(page);

  await expect(page.getByText("flat-1")).toBeVisible();
  await expect(page.getByText("flat-2")).toBeVisible();
  // Conditional rule: a single distinct repo ⇒ no group header (flat, as before).
  await expect(page.getByTestId("trigger-repo-group")).toHaveCount(0);
});

test("Triggers list groups by repo across ≥2 repos; null target resolves to the daemon repo, no Unassigned", async ({
  page,
  baseURL,
}) => {
  await createTrigger(page, baseURL!, "g-a", repoA);
  await createTrigger(page, baseURL!, "g-b", repoB);
  await createTrigger(page, baseURL!, "g-null", null); // resolves to WORKSPACE_ROOT

  await openTriggersTab(page);
  await expect(page.getByText("g-a")).toBeVisible();

  // Three distinct repos ⇒ three group headers.
  await expect(page.getByTestId("trigger-repo-group")).toHaveCount(3);

  const labels = await page.getByTestId("trigger-repo-label").allTextContents();
  expect(labels).toContain(path.basename(repoA));
  expect(labels).toContain(path.basename(repoB));
  // The null-target trigger grouped under the daemon's own repo (basename), not
  // a separate "Unassigned" bucket.
  expect(labels).toContain(path.basename(WORKSPACE_ROOT));
  expect(labels.join(" ")).not.toMatch(/unassigned/i);

  // Groups are ordered by full PATH (groupByRepo), not by the basename label.
  // Assert the displayed label order equals sorting the three repos by full path,
  // so the check is robust to the workspace's absolute path — which differs
  // between local (…/Maestro) and CI (…/prompt-driven-orchestrator) and would
  // otherwise flip basename order relative to full-path order.
  const expectedLabelOrder = [repoA, repoB, WORKSPACE_ROOT]
    .sort((a, b) => (a < b ? -1 : a > b ? 1 : 0))
    .map((p) => path.basename(p));
  expect(labels).toEqual(expectedLabelOrder);

  // Full path on hover: the repoA group header carries title=repoA.
  const repoALabel = page
    .getByTestId("trigger-repo-label")
    .filter({ hasText: path.basename(repoA) });
  await expect(repoALabel.locator("xpath=..")).toHaveAttribute("title", repoA);

  // Raw target_repo unchanged: g-null shows NO badge; g-a shows a badge titled repoA.
  const nullRow = page.getByTestId("trigger-row").filter({ hasText: "g-null" });
  await expect(nullRow.locator(`[title="${WORKSPACE_ROOT}"]`)).toHaveCount(0);
  const aRow = page.getByTestId("trigger-row").filter({ hasText: "g-a" });
  await expect(aRow.locator(`[title="${repoA}"]`)).toHaveCount(1);
});

test("Runs list groups by repo when runs span ≥2 repos", async ({ page, baseURL }) => {
  // Seed one run in each of two distinct repos (worker session is the e2e sleep stub).
  const seededIds: string[] = [];
  for (const repo of [repoA, repoB]) {
    const resp = await page.request.post(`${baseURL}/runs`, {
      data: { pipeline: PIPELINE_NAME, input: "seed", target_repo: repo },
    });
    expect(resp.status()).toBe(201);
    seededIds.push(((await resp.json()) as { run_id: string }).run_id);
  }

  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });
  // Runs is the default tab. Wait for this spec's rows before sampling the
  // group headers: count()/allTextContents() don't auto-wait, and under
  // parallel-suite load the initial /runs fetch can land after first paint.
  for (const id of seededIds) {
    await expect(page.getByText(id.slice(0, 20))).toBeVisible({ timeout: 10_000 });
  }
  const labels = await page.getByTestId("run-repo-label").allTextContents();
  expect(await page.getByTestId("run-repo-group").count()).toBeGreaterThanOrEqual(2);
  expect(labels).toContain(path.basename(repoA));
  expect(labels).toContain(path.basename(repoB));
  // Run rows carry no per-row repo badge — the header is the only repo surface (G2).
  // (The trigger-provenance badge is unrelated and absent for these manual runs.)
  // Scoped to THIS spec's rows: parallel spec files may hold triggered runs
  // whose provenance badge is legitimate (e.g. runs-filter, #336).
  for (const id of seededIds) {
    const row = page.getByRole("button").filter({ hasText: id.slice(0, 20) });
    await expect(row.getByTestId("run-trigger-badge")).toHaveCount(0);
  }
});
