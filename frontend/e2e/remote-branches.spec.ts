import { test, expect } from "@playwright/test";
import { execSync } from "node:child_process";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { cleanupRuns } from "./helpers";

// Layer 3b (real browser ↔ real daemon) for #571. The New Run source-branch
// select must offer remote-tracking refs in a "Remote" group, default to a local
// branch, and post the chosen remote ref VERBATIM (`origin/…`, no stripping) —
// all resolved locally, no `git fetch`. This is the only level that proves the
// modal→daemon contract end to end without a mock.
//
// The pipeline is seeded into the DAEMON's workspace (fetchPipelines is
// instance-level, not per-target-repo), the git fixture into os.tmpdir(), and
// the Run is pointed at that fixture.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-remote-branches-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);
const PROMPTS_DIR = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.prompts`);

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
    prompt_file: ${PIPELINE_NAME}.prompts/worker.md
    inputs:
      - { name: task, side: top }
    outputs:
      - { name: result, side: bottom }
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
  - source: { node: worker, port: result }
    target: { node: end, port: result }
`;

const ROOT = path.join(
  os.tmpdir(),
  `pdo-e2e-remote-branches-${process.pid}-${Date.now()}`,
);
const ORIGIN = path.join(ROOT, "origin.git");
const WORK = path.join(ROOT, "work");

let createdRunId: string | undefined;

test.beforeAll(() => {
  // Pipeline seed (instance-level, so it shows for any target repo).
  fs.mkdirSync(PROMPTS_DIR, { recursive: true });
  fs.writeFileSync(PIPELINE_PATH, SEED_YAML);
  fs.writeFileSync(path.join(PROMPTS_DIR, "worker.md"), "You are a worker.\n");

  // Git fixture over a filesystem "remote" — zero network. End state of WORK:
  // local `main` + `local-branch`; remote-tracking `origin/main` (twin of local
  // → deduped away), `origin/feature-remote-only` (remote-only → must surface),
  // `origin/HEAD` (symref → must never surface).
  fs.rmSync(ROOT, { recursive: true, force: true });
  fs.mkdirSync(ROOT, { recursive: true });
  const sh = (cmd: string, cwd: string) => execSync(cmd, { cwd, stdio: "ignore" });
  execSync(`git init --bare -b main "${ORIGIN}"`, { stdio: "ignore" });
  execSync(`git clone -q "${ORIGIN}" "${WORK}"`, { stdio: "ignore" });
  sh("git config user.email t@t.co && git config user.name t", WORK);
  sh("echo hi > README.md && git add . && git commit -qm init && git push -q -u origin main", WORK);
  sh(
    "git checkout -qb feature-remote-only && echo x > x.txt && git add . && " +
      "git commit -qm feat && git push -q -u origin feature-remote-only",
    WORK,
  );
  sh("git checkout -q main && git branch -qD feature-remote-only", WORK);
  sh("git checkout -qb local-branch && git commit -qm empty --allow-empty && git checkout -q main", WORK);
});

test.afterAll(async () => {
  await cleanupRuns(createdRunId);
  fs.rmSync(ROOT, { recursive: true, force: true });
  fs.rmSync(PIPELINE_PATH, { force: true });
  fs.rmSync(PROMPTS_DIR, { recursive: true, force: true });
});

test("offers remote branches grouped, defaults local, launches one verbatim", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  await page.getByRole("button", { name: "New Run" }).click();
  await expect(page.getByTestId("target-repo-input")).toBeVisible();
  await page.getByTestId("target-repo-input").fill(WORK);

  // The select appears once the repo validates and its branches load.
  const branchSelect = page.getByTestId("source-branch-select");
  await expect(branchSelect).toBeVisible({ timeout: 10_000 });

  // Two groups: Local (main, local-branch) and Remote (origin/feature-remote-only).
  await expect(branchSelect.locator('optgroup[label="Local"]')).toHaveCount(1);
  await expect(branchSelect.locator('optgroup[label="Remote"]')).toHaveCount(1);
  await expect(branchSelect.locator('option[value="main"]')).toHaveCount(1);
  await expect(branchSelect.locator('option[value="local-branch"]')).toHaveCount(1);
  await expect(
    branchSelect.locator('option[value="origin/feature-remote-only"]'),
  ).toHaveCount(1);

  // Deduped / filtered out: no origin/main twin, no symref.
  await expect(branchSelect.locator('option[value="origin/main"]')).toHaveCount(0);
  await expect(branchSelect.locator('option[value="origin/HEAD"]')).toHaveCount(0);
  await expect(branchSelect.locator('option[value="origin"]')).toHaveCount(0);

  // Default lands on the LOCAL main — never a remote while a local exists.
  await expect(branchSelect).toHaveValue("main");

  // Launch from the remote-only ref.
  await branchSelect.selectOption("origin/feature-remote-only");
  await page.getByTestId("pipeline-select").selectOption({ label: PIPELINE_NAME });
  await page.getByPlaceholder(/free-text prompt/i).fill("remote branch e2e");
  await expect(page.getByTestId("launch-button")).toBeEnabled();
  await page.getByTestId("launch-button").click();

  // The daemon accepted it (no 400 `branch … does not exist`) — a Run against the
  // fixture repo appears. The list entry carries no `source_branch` (it lives on
  // the run detail), so match on the unique fixture repo, then read the detail.
  await expect
    .poll(
      async () => {
        const resp = await page.request.get(`${baseURL}/runs`);
        const runs = (await resp.json()) as Array<{
          run_id: string;
          effective_repo?: string;
        }>;
        const hit = runs.find((r) => r.effective_repo === WORK);
        createdRunId = hit?.run_id;
        return hit ? "found" : "absent";
      },
      { timeout: 15_000 },
    )
    .toBe("found");

  // The chosen remote ref was stored VERBATIM — prefix and all — on the run.
  const detail = await (
    await page.request.get(`${baseURL}/runs/${createdRunId}`)
  ).json();
  expect(detail.source_branch).toBe("origin/feature-remote-only");

  // No local branch was materialised from the remote-only source.
  const localBranches = execSync('git branch --list feature-remote-only', {
    cwd: WORK,
    encoding: "utf8",
  }).trim();
  expect(localBranches).toBe("");
});
