import { test, expect, type Locator, type Page } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as os from "node:os";
import * as path from "node:path";
import { execFileSync } from "node:child_process";

// Layer 4 (e2e) per ADR 0004 — proves #940 in a real browser at a LOW window
// (1280×600): the Edit project modal is bounded to the window like New Run. Its
// title and Save stay in the viewport, only the body scrolls, down to the last
// member repository. jsdom computes no layout, so this can only be pinned here.
//
// The pencil lives on the Triggers list group headers too (#552); Triggers are
// the lightest way to put several repos in a list (no node session spawned).

const PREFIX = "e2e-bounded-";
const PIPELINE_NAME = `${PREFIX}${process.pid}-${Date.now()}`;
// Instance pipelines live under `$HOME/.pdo/pipelines` — the daemon's instance
// root — not under the workspace's `.pdo/` (the Run's blackboard).
const PIPELINE_DIR = path.join(os.homedir(), ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);
const PROJECT_NAME = `${PREFIX}project-${process.pid}`;
const REPO_COUNT = 6;

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
  - id: end
    name: End
    type: end
    inputs:
      - name: result
    view: { x: 200, y: 100 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: end, port: result }
`;

let repos: string[] = [];

function gitInit(dir: string) {
  execFileSync("git", ["init", "-b", "main"], { cwd: dir });
  execFileSync("git", ["config", "user.email", "t@t.c"], { cwd: dir });
  execFileSync("git", ["config", "user.name", "t"], { cwd: dir });
  execFileSync("git", ["commit", "--allow-empty", "-m", "init"], { cwd: dir });
}

test.use({ viewport: { width: 1280, height: 600 } });

test.beforeAll(async () => {
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
  repos = [];
  for (let i = 0; i < REPO_COUNT; i++) {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), `pdo940-${i}-`));
    gitInit(dir);
    repos.push(dir);
  }
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  for (const dir of repos) await fs.rm(dir, { recursive: true, force: true });
});

// Scoped cleanup (never a global wipe): other specs share this daemon.
async function cleanup(page: Page, baseURL: string) {
  const triggers = (await (await page.request.get(`${baseURL}/triggers`)).json()) as Array<{
    id: string;
    pipeline_id: string;
  }>;
  for (const t of triggers) {
    if (t.pipeline_id.startsWith(PREFIX)) await page.request.delete(`${baseURL}/triggers/${t.id}`);
  }
  const projects = (await (await page.request.get(`${baseURL}/projects`)).json()) as Array<{
    id: string;
    name: string;
  }>;
  for (const p of projects) {
    if (p.name.startsWith(PREFIX)) await page.request.delete(`${baseURL}/projects/${p.id}`);
  }
}

test.beforeEach(async ({ page, baseURL }) => {
  // A daemon with no Run offers the welcome tour (#823), whose backdrop would
  // swallow every click: answer it up front, as a returning reader has.
  await page.addInitScript(() => localStorage.setItem("pdo.tour.offered", "1"));
  await cleanup(page, baseURL!);
  for (const [i, repo] of repos.entries()) {
    const resp = await page.request.post(`${baseURL}/triggers`, {
      data: { name: `${PREFIX}t${i}`, pipeline_id: PIPELINE_NAME, cron: "0 0 1 1 *", target_repo: repo },
    });
    expect(resp.status(), `POST /triggers ${i}`).toBe(201);
  }
});

test.afterEach(async ({ page, baseURL }) => {
  await cleanup(page, baseURL!);
});

async function openTriggersTab(page: Page) {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });
  await page.getByRole("tab", { name: "Triggers" }).click();
  await expect(page.getByText(`${PREFIX}t0`)).toBeVisible();
}

/** Hover a group header (the pencil only shows on hover) and click its pencil. */
async function openPencil(page: Page, label: string) {
  const group = page
    .getByTestId("trigger-repo-group")
    .filter({ has: page.getByTestId("trigger-repo-label").getByText(label, { exact: true }) });
  const header = group.getByTestId("trigger-repo-label");
  await header.scrollIntoViewIfNeeded();
  await header.hover();
  await group.getByTestId("trigger-group-pencil").click();
}

async function overflows(body: Locator): Promise<boolean> {
  return body.evaluate((el) => el.scrollHeight > el.clientHeight);
}

test("Edit project with provisioning open stays in the window; the body scrolls to the last member", async ({
  page,
  baseURL,
}) => {
  const created = await page.request.post(`${baseURL}/projects`, { data: { name: PROJECT_NAME } });
  expect(created.status()).toBeLessThan(300);
  const project = (await created.json()) as { id: string };
  const attach = await page.request.post(`${baseURL}/projects/${project.id}/members`, {
    data: { path: repos[0] },
  });
  expect(attach.status()).toBeLessThan(300);

  await openTriggersTab(page);
  await openPencil(page, PROJECT_NAME);

  const modal = page.getByTestId("project-edit-modal");
  await expect(modal).toBeVisible();
  await expect(modal.getByText("Edit project", { exact: true })).toBeInViewport();
  await expect(page.getByTestId("project-edit-save")).toBeInViewport();

  await modal.getByRole("button", { name: "Configure worktree provisioning…" }).click();
  await expect(modal.getByTestId("provisioning-project")).toBeVisible();

  // The panel never leaves the window: bounded to 85vh (510px at 600px).
  const box = (await modal.boundingBox())!;
  expect(box.y).toBeGreaterThanOrEqual(0);
  expect(box.y + box.height).toBeLessThanOrEqual(600);
  expect(box.height).toBeLessThanOrEqual(0.85 * 600 + 1);

  // Only the body scrolls; title and Save stay put.
  const body = page.getByTestId("project-edit-body");
  expect(await overflows(body)).toBe(true);
  await expect(modal.getByText("Edit project", { exact: true })).toBeInViewport();
  await expect(page.getByTestId("project-edit-save")).toBeInViewport();

  // The member list keeps its own bounded height, nested in the scrolling body.
  const members = page.getByTestId("project-members-list");
  expect((await members.boundingBox())!.height).toBeLessThanOrEqual(192 + 1);

  // Scroll down to the last member repository, check a new one, save.
  const target = members.locator(`[data-testid="project-member-row"][data-path="${repos[REPO_COUNT - 1]}"]`);
  await target.scrollIntoViewIfNeeded();
  await expect(target).toBeInViewport();
  await expect(page.getByTestId("project-edit-save")).toBeInViewport();
  await target.getByTestId("project-member-checkbox").check();
  await page.getByTestId("project-edit-save").click();
  await expect(modal).toHaveCount(0);

  // The repo now sits under the Projet's group in the Triggers list.
  const group = page
    .getByTestId("trigger-repo-group")
    .filter({ has: page.getByTestId("trigger-repo-label").getByText(PROJECT_NAME, { exact: true }) });
  await expect(group.getByText(`${PREFIX}t${REPO_COUNT - 1}`)).toBeVisible();
});

test("Name project with short content: no scrollbar, rendered as before", async ({ page }) => {
  // "Short" is relative to the window: the member list alone (up to 192px) fills
  // 85vh at 600px, so give this one a window the content genuinely fits in.
  await page.setViewportSize({ width: 1280, height: 900 });
  await openTriggersTab(page);
  await openPencil(page, path.basename(repos[1]));

  const modal = page.getByTestId("project-edit-modal");
  await expect(modal.getByText("Name project", { exact: true })).toBeInViewport();
  await expect(page.getByTestId("project-edit-save")).toBeInViewport();
  expect(await overflows(page.getByTestId("project-edit-body"))).toBe(false);

  // Backdrop click behaviour is unchanged: it still closes the modal.
  await page.getByTestId("project-edit-backdrop").click({ position: { x: 5, y: 5 } });
  await expect(modal).toHaveCount(0);
});
