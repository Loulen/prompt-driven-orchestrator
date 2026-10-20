import { test, expect, type Page } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as os from "node:os";
import * as path from "node:path";
import { execFileSync } from "node:child_process";

// Layer 4 (e2e) per ADR 0004 — CORE-96: Settings › Projects in a real browser over a
// real daemon. Create a Projet, add a local repository through the explorer and a new
// empty one (`git init` + first commit, checked on disk), choose the default primary,
// and New Run preselects it. A second browser sees the changes live.

const PREFIX = "e2e-settings-projects-";
const PROJECT_NAME = `${PREFIX}widgets-${process.pid}`;
// Under the daemon's home: the explorer opens there.
const ROOT = path.join(os.homedir(), `${PREFIX}${process.pid}-${Date.now()}`);
const LOCAL = path.join(ROOT, "local-repo");
const FRESH = path.join(ROOT, "fresh-repo");
const OCCUPIED = path.join(ROOT, "occupied");

function git(dir: string, args: string[]): string {
  return execFileSync("git", args, { cwd: dir }).toString().trim();
}

test.beforeAll(async () => {
  await fs.mkdir(LOCAL, { recursive: true });
  git(LOCAL, ["init", "-b", "main"]);
  git(LOCAL, ["config", "user.email", "t@t.c"]);
  git(LOCAL, ["config", "user.name", "t"]);
  git(LOCAL, ["remote", "add", "origin", "https://example.com/acme/local-repo.git"]);
  git(LOCAL, ["commit", "--allow-empty", "-m", "init"]);
  await fs.mkdir(OCCUPIED, { recursive: true });
  await fs.writeFile(path.join(OCCUPIED, "notes.txt"), "mine");
});

test.afterAll(async () => {
  await fs.rm(ROOT, { recursive: true, force: true });
});

// Scoped cleanup (never a global wipe): other specs share this daemon.
async function cleanup(page: Page, baseURL: string) {
  const projects = (await (await page.request.get(`${baseURL}/projects`)).json()) as Array<{
    id: string;
    name: string;
  }>;
  for (const p of projects) {
    if (p.name.startsWith(PREFIX)) await page.request.delete(`${baseURL}/projects/${p.id}`);
  }
}

test.beforeEach(async ({ page, baseURL }) => {
  await cleanup(page, baseURL!);
});

test.afterEach(async ({ page, baseURL }) => {
  await cleanup(page, baseURL!);
});

async function openProjectsSettings(page: Page) {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });
  await page.getByTestId("open-settings").click();
  await page.getByTestId("settings-category-projects").click();
  await expect(page.getByTestId("projects-panel")).toBeVisible();
}

const repoRow = (page: Page, repo: string) =>
  page.locator(`[data-testid="project-repo-row"][data-path="${repo}"]`);

test("create a Projet, add a local and a new empty repository, choose the default primary", async ({
  page,
  browser,
}) => {
  await openProjectsSettings(page);
  // A second browser on the same page sees every change live.
  const other = await browser.newPage();
  await openProjectsSettings(other);

  await page.getByTestId("projects-new").click();
  await page.getByTestId("projects-new-name").fill(PROJECT_NAME);
  await page.getByTestId("projects-new-name").press("Enter");
  await expect(page.getByTestId("project-detail-name")).toHaveText(PROJECT_NAME);
  await expect(page.getByTestId("project-detail-runs")).toHaveText("0 runs");
  await expect(other.getByTestId("projects-list-item").filter({ hasText: PROJECT_NAME })).toBeVisible();

  // A local repository, through the explorer (it opens in the daemon's home).
  await page.getByTestId("project-add-repo").click();
  await page.getByTestId("project-add-repo-local").click();
  await page.getByTestId("fs-browse-entry").filter({ hasText: path.basename(ROOT) }).click();
  await expect(page.getByTestId("fs-browse-path")).toHaveText(ROOT);
  await page.getByTestId("fs-browse-entry").filter({ hasText: "local-repo" }).click();
  await page.getByTestId("fs-browse-select").click();
  await expect(repoRow(page, LOCAL)).toBeVisible();
  await expect(repoRow(page, LOCAL).getByTestId("project-repo-remote")).toHaveText(
    "https://example.com/acme/local-repo.git",
  );
  // The first repository is the default primary until another is chosen.
  await expect(repoRow(page, LOCAL).getByTestId("project-repo-primary")).toBeVisible();

  // A folder that holds something and is not a repository is refused, by name.
  await page.getByTestId("project-add-repo").click();
  await page.getByTestId("project-add-repo-new").click();
  await page.getByTestId("project-new-repo-path").fill(OCCUPIED);
  await page.getByTestId("project-new-repo-create").click();
  await expect(page.getByTestId("project-new-repo-error")).toContainText("exists and is not a git repository");

  // A new empty repository: created with its first commit, then added.
  await page.getByTestId("project-new-repo-path").fill(FRESH);
  await page.getByTestId("project-new-repo-create").click();
  await expect(repoRow(page, FRESH)).toBeVisible();
  expect(git(FRESH, ["rev-list", "--count", "HEAD"])).toBe("1");
  expect(git(FRESH, ["branch", "--show-current"])).toBe("main");
  await expect(repoRow(page, FRESH).getByTestId("project-repo-remote")).toHaveText("no remote");

  // The default primary: the new repository.
  await repoRow(page, FRESH).hover();
  await repoRow(page, FRESH).getByTestId("project-repo-make-primary").click();
  await expect(repoRow(page, FRESH).getByTestId("project-repo-primary")).toBeVisible();
  await expect(repoRow(page, LOCAL).getByTestId("project-repo-primary")).toHaveCount(0);

  // Live in the other browser.
  await other.getByTestId("projects-list-item").filter({ hasText: PROJECT_NAME }).click();
  await expect(other.locator(`[data-testid="project-repo-row"][data-path="${FRESH}"]`).getByTestId("project-repo-primary")).toBeVisible();
  await other.close();

  // New Run in this Projet preselects its default primary.
  await page.getByRole("button", { name: "Close settings" }).click();
  const projects = (await (await page.request.get("/projects")).json()) as Array<{ id: string; name: string }>;
  const id = projects.find((p) => p.name === PROJECT_NAME)!.id;
  await page.getByRole("tab", { name: "Runs" }).click();
  await page.getByRole("button", { name: "New Run" }).click();
  await page.getByTestId("project-combobox").click();
  await page.locator(`[data-testid="project-option"][data-project-id="${id}"]`).click();
  await expect(page.getByTestId("target-repo-input")).toHaveValue(FRESH);
  await expect(page.getByTestId("project-default-badge")).toBeVisible();
});

test("rename and delete a Projet", async ({ page, baseURL }) => {
  const created = await page.request.post(`${baseURL}/projects`, {
    data: { name: `${PROJECT_NAME}-a`, members: [LOCAL] },
  });
  expect(created.status()).toBeLessThan(300);

  await openProjectsSettings(page);
  await page.getByTestId("projects-list-item").filter({ hasText: `${PROJECT_NAME}-a` }).click();
  await page.getByTestId("project-rename").click();
  await page.getByTestId("project-rename-input").fill(`${PROJECT_NAME}-b`);
  await page.getByTestId("project-rename-input").press("Enter");
  await expect(page.getByTestId("project-detail-name")).toHaveText(`${PROJECT_NAME}-b`);

  await page.getByTestId("project-delete").click();
  await expect(page.getByTestId("confirm-delete-modal")).toContainText("Its Runs keep it, shown as deleted.");
  await page.getByTestId("confirm-delete-modal").getByRole("button", { name: "Delete" }).click();
  await expect(page.getByTestId("projects-list-item").filter({ hasText: `${PROJECT_NAME}-b` })).toHaveCount(0);
  // Settings stays open: the confirmation's Escape and click are its own.
  await expect(page.getByTestId("projects-panel")).toBeVisible();
});
