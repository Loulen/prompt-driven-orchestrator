import { test, expect, type Page } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as os from "node:os";
import * as path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { cleanupRuns } from "./helpers";

// Layer 4 (e2e) per ADR 0004 — proves #336 end-to-end in a browser: the Runs
// list carries three client-side filter dropdowns (Project / Pipeline /
// Trigger) with AND semantics, an "All" default, a Manual trigger option, and a
// clear control. Filtering also drives the #258 grouped/flat flip.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const TAG = `${process.pid}-${Date.now()}`;
const PIPE_A = `e2e-filter-a-${TAG}`;
const PIPE_B = `e2e-filter-b-${TAG}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".pdo", "pipelines");

// `prompt_required: false` so a cron-only trigger with no input template is
// accepted at creation (same seed as runs-triggers-by-repo.spec.ts).
function seedYaml(name: string): string {
  return `name: ${name}
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
    prompt_file: ${name}.prompts/worker.md
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
}

let repoA = "";
let repoB = "";
const runIds: string[] = [];
let triggerId = "";

function gitInit(dir: string) {
  execFileSync("git", ["init", "-b", "main"], { cwd: dir });
  execFileSync("git", ["config", "user.email", "t@t.c"], { cwd: dir });
  execFileSync("git", ["config", "user.name", "t"], { cwd: dir });
  execFileSync("git", ["commit", "--allow-empty", "-m", "init"], { cwd: dir });
}

test.beforeAll(async () => {
  for (const name of [PIPE_A, PIPE_B]) {
    const promptsDir = path.join(PIPELINE_DIR, `${name}.prompts`);
    await fs.mkdir(promptsDir, { recursive: true });
    await fs.writeFile(path.join(PIPELINE_DIR, `${name}.yaml`), seedYaml(name));
    await fs.writeFile(path.join(promptsDir, "worker.md"), "You are a worker.\n");
  }
  repoA = await fs.mkdtemp(path.join(os.tmpdir(), "pdo336-alpha-"));
  repoB = await fs.mkdtemp(path.join(os.tmpdir(), "pdo336-beta-"));
  gitInit(repoA);
  gitInit(repoB);
});

test.afterAll(async () => {
  // Best-effort trigger teardown here (not inline at the end of the test) so a
  // mid-test failure never leaks the trigger into other specs' daemon state.
  if (triggerId) {
    const base = `http://127.0.0.1:${Number(process.env.PDO_E2E_PORT ?? 5273)}`;
    await fetch(`${base}/triggers/${triggerId}`, { method: "DELETE" }).catch(() => {});
  }
  await cleanupRuns(...runIds);
  for (const name of [PIPE_A, PIPE_B]) {
    await fs.rm(path.join(PIPELINE_DIR, `${name}.yaml`), { force: true });
    await fs.rm(path.join(PIPELINE_DIR, `${name}.prompts`), {
      recursive: true,
      force: true,
    });
  }
  if (repoA) await fs.rm(repoA, { recursive: true, force: true });
  if (repoB) await fs.rm(repoB, { recursive: true, force: true });
});

async function seedRun(
  page: Page,
  baseURL: string,
  pipeline: string,
  repo: string,
  name: string,
  triggeredBy?: string,
): Promise<string> {
  const data: Record<string, unknown> = {
    pipeline,
    input: "seed",
    target_repo: repo,
    name,
  };
  if (triggeredBy) data.triggered_by = triggeredBy;
  const resp = await page.request.post(`${baseURL}/runs`, { data });
  expect(resp.status(), `POST /runs ${name}`).toBe(201);
  const { run_id } = (await resp.json()) as { run_id: string };
  runIds.push(run_id);
  return run_id;
}

async function visibleRunNames(page: Page): Promise<string[]> {
  return page.getByTestId("run-display-label").allTextContents();
}

test("Runs list filters by project, pipeline and trigger with AND semantics and a clear control", async ({
  page,
  baseURL,
}) => {
  // A real trigger so the Trigger dropdown resolves its id to a name.
  const trigResp = await page.request.post(`${baseURL}/triggers`, {
    data: { name: `nightly-${TAG}`, pipeline_id: PIPE_A, cron: "0 0 1 1 *" },
  });
  expect(trigResp.status()).toBe(201);
  triggerId = ((await trigResp.json()) as { id: string }).id;

  // 2 repos × 2 pipelines, 2 manual runs + 1 triggered run.
  await seedRun(page, baseURL!, PIPE_A, repoA, `alpha-a-${TAG}`);
  await seedRun(page, baseURL!, PIPE_B, repoA, `alpha-b-${TAG}`);
  await seedRun(page, baseURL!, PIPE_A, repoB, `beta-a-${TAG}`, triggerId);

  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  // Filter row present with "All" placeholders (Runs is the default tab). Other
  // specs' residual runs may coexist on the shared e2e daemon, so assertions
  // use this spec's uniquely-named rows, never absolute counts.
  await expect(page.getByTestId("run-filter-project")).toBeVisible();
  await expect(page.getByTestId("run-filter-pipeline")).toBeVisible();
  await expect(page.getByTestId("run-filter-trigger")).toBeVisible();
  await expect(page.getByText(`alpha-a-${TAG}`)).toBeVisible();
  await expect(page.getByText(`alpha-b-${TAG}`)).toBeVisible();
  await expect(page.getByText(`beta-a-${TAG}`)).toBeVisible();

  // Pipeline filter: only PIPE_A runs remain.
  await page.getByTestId("run-filter-pipeline").click();
  await page.getByTestId(`run-filter-option-${PIPE_A}`).click();
  let names = await visibleRunNames(page);
  expect(names).toContain(`alpha-a-${TAG}`);
  expect(names).toContain(`beta-a-${TAG}`);
  expect(names).not.toContain(`alpha-b-${TAG}`);

  // AND with the Project filter: repoB + PIPE_A ⇒ only the triggered run. A
  // single remaining repo also flips the grouped list flat for these rows.
  await page.getByTestId("run-filter-project").click();
  await page.getByTestId(`run-filter-option-${repoB}`).click();
  names = await visibleRunNames(page);
  expect(names).toContain(`beta-a-${TAG}`);
  expect(names).not.toContain(`alpha-a-${TAG}`);
  expect(names).not.toContain(`alpha-b-${TAG}`);

  // Trigger filter resolves the trigger id to its NAME in the menu.
  await page.getByTestId("run-filter-trigger").click();
  const trigOption = page.getByTestId(`run-filter-option-${triggerId}`);
  await expect(trigOption).toHaveText(`nightly-${TAG}`);
  await trigOption.click();
  names = await visibleRunNames(page);
  expect(names).toContain(`beta-a-${TAG}`);

  // Contradictory AND: Manual + repoB + PIPE_A ⇒ zero rows ⇒ empty state.
  await page.getByTestId("run-filter-trigger").click();
  await page.getByTestId("run-filter-option-__manual__").click();
  await expect(page.getByTestId("run-filter-empty")).toBeVisible();

  // Clear restores everything.
  await page.getByTestId("run-filter-clear").click();
  await expect(page.getByTestId("run-filter-empty")).toHaveCount(0);
  await expect(page.getByText(`alpha-a-${TAG}`)).toBeVisible();
  await expect(page.getByText(`alpha-b-${TAG}`)).toBeVisible();
  await expect(page.getByText(`beta-a-${TAG}`)).toBeVisible();
});
