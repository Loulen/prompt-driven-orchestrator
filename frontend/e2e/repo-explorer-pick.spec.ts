import { test, expect } from "@playwright/test";
import { execSync } from "node:child_process";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

// Layer 3b (real browser ↔ real daemon) for the #131 filesystem explorer. The
// explorer browses the *real* filesystem, so we seed a deterministic tree on disk
// the assertions bind to, then drive the loupe → navigate → pick flow and assert the
// pick reuses the existing validation path (green border + branch loading).

const ROOT = path.join(os.tmpdir(), `pdo-e2e-repo-explorer-${process.pid}-${Date.now()}`);
const ALPHA = path.join(ROOT, "alpha-project");
const BETA = path.join(ROOT, "beta-plain");

test.beforeAll(() => {
  fs.rmSync(ROOT, { recursive: true, force: true });
  fs.mkdirSync(ALPHA, { recursive: true });
  // A real git repo with a commit on `main` so /repos/branches returns a branch.
  execSync(
    "git init -b main && git config user.email t@t.co && git config user.name t && " +
      "echo hi > README.md && git add . && git commit -qm init",
    { cwd: ALPHA },
  );
  fs.mkdirSync(BETA); // plain dir, no .git → validates RED
  fs.mkdirSync(path.join(ROOT, ".hidden-dir")); // dotfile → hidden
  fs.symlinkSync(ALPHA, path.join(ROOT, "zeta-link")); // symlink → listed + flagged
  fs.writeFileSync(path.join(ROOT, "notes.txt"), "notes"); // file → filtered out
});

test.afterAll(() => {
  fs.rmSync(ROOT, { recursive: true, force: true });
});

test("explorer lists dirs-only, navigates, and picks a git repo through validation", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  await page.getByRole("button", { name: "New Run" }).click();
  await expect(page.getByTestId("target-repo-input")).toBeVisible();

  // Open-at (Option B): type the seed root, then open the explorer there.
  await page.getByTestId("target-repo-input").fill(ROOT);
  await page.getByTestId("repo-browse-trigger").click();

  await expect(page.getByTestId("repo-browser-modal")).toBeVisible();
  await expect(page.getByTestId("repo-browse-path")).toHaveText(ROOT);

  // Listing: dirs only, dotfile + file excluded, alpha-sorted.
  const entries = page.getByTestId("repo-browse-entry");
  await expect(entries).toHaveCount(3);
  await expect(entries.nth(0)).toContainText("alpha-project");
  await expect(entries.nth(1)).toContainText("beta-plain");
  await expect(entries.nth(2)).toContainText("zeta-link");
  await expect(page.getByText(".hidden-dir")).toHaveCount(0);
  await expect(page.getByText("notes.txt")).toHaveCount(0);
  // alpha-project is git-flagged; zeta-link is symlink-flagged.
  await expect(page.getByTestId("repo-browse-git-dot")).toHaveCount(1);
  await expect(page.getByTestId("repo-browse-symlink")).toHaveCount(1);

  // Navigate into the git repo, then pick the current folder.
  await entries.nth(0).click();
  await expect(page.getByTestId("repo-browse-path")).toHaveText(ALPHA);
  await page.getByTestId("repo-browse-select").click();

  // Explorer closes; the New Run modal stays open; the pick flowed through onChange.
  await expect(page.getByTestId("repo-browser-modal")).toBeHidden();
  await expect(page.getByTestId("target-repo-input")).toHaveValue(ALPHA);

  // The pick reused the existing validation/branch-loading flow — no new logic.
  await expect(page.getByTestId("repo-valid")).toBeVisible({ timeout: 10_000 });
  await expect(page.getByTestId("source-branch-select")).toBeVisible();
  await expect(page.getByTestId("source-branch-select")).toContainText("main");
});

test("picking a non-git folder validates red (any folder pickable, git gates)", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  await page.getByRole("button", { name: "New Run" }).click();
  await expect(page.getByTestId("target-repo-input")).toBeVisible();

  await page.getByTestId("target-repo-input").fill(ROOT);
  await page.getByTestId("repo-browse-trigger").click();
  await expect(page.getByTestId("repo-browser-modal")).toBeVisible();

  // beta-plain has no .git → pickable, but validates red.
  await page.getByTestId("repo-browse-entry").nth(1).click();
  await expect(page.getByTestId("repo-browse-path")).toHaveText(BETA);
  await page.getByTestId("repo-browse-select").click();

  await expect(page.getByTestId("target-repo-input")).toHaveValue(BETA);
  await expect(page.getByTestId("repo-error")).toBeVisible({ timeout: 10_000 });
  await expect(page.getByTestId("repo-valid")).toBeHidden();
});

test("nested modal: Escape closes only the explorer, never the parent", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  await page.getByRole("button", { name: "New Run" }).click();
  await expect(page.getByTestId("target-repo-input")).toBeVisible();

  await page.getByTestId("target-repo-input").fill(ROOT);
  await page.getByTestId("repo-browse-trigger").click();
  await expect(page.getByTestId("repo-browser-modal")).toBeVisible();

  await page.keyboard.press("Escape");
  await expect(page.getByTestId("repo-browser-modal")).toBeHidden();
  // The parent New Run modal must still be open.
  await expect(page.getByTestId("target-repo-input")).toBeVisible();

  // Backdrop click also closes only the explorer.
  await page.getByTestId("repo-browse-trigger").click();
  await expect(page.getByTestId("repo-browser-modal")).toBeVisible();
  await page.getByTestId("repo-browse-backdrop").click({ position: { x: 5, y: 5 } });
  await expect(page.getByTestId("repo-browser-modal")).toBeHidden();
  await expect(page.getByTestId("target-repo-input")).toBeVisible();
});
