import { test, expect, type Locator, type Page } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as os from "node:os";
import * as path from "node:path";
import { execFileSync } from "node:child_process";

// Layer 4 (e2e) per ADR 0004 — CORE-108: in New Run, a Projet's default repository
// with a 150-character path. The path is cut in its middle (its last segment
// whole), the « default » mark and the loupe sit beside it, never over it. The
// modal has one width (480px), its narrowest. jsdom computes no layout, so this
// can only be pinned here.

const PREFIX = "e2e-target-repo-";
const PROJECT_NAME = `${PREFIX}${process.pid}`;
const ROOT = path.join(os.homedir(), `${PREFIX}${process.pid}-${Date.now()}`);
const LAST = "frontend-application";
// Padded so the whole path is 150 characters.
const DEEP = path.join(
  ROOT,
  "x".repeat(150 - ROOT.length - LAST.length - 2),
  LAST,
);
const OTHER = path.join(ROOT, "shop-api");

function gitInit(dir: string) {
  execFileSync("git", ["init", "-b", "main"], { cwd: dir });
  execFileSync("git", ["config", "user.email", "t@t.c"], { cwd: dir });
  execFileSync("git", ["config", "user.name", "t"], { cwd: dir });
  execFileSync("git", ["commit", "--allow-empty", "-m", "init"], { cwd: dir });
}

test.beforeAll(async () => {
  expect(DEEP).toHaveLength(150);
  for (const dir of [DEEP, OTHER]) {
    await fs.mkdir(dir, { recursive: true });
    gitInit(dir);
  }
});

test.afterAll(async () => {
  await fs.rm(ROOT, { recursive: true, force: true });
});

// Scoped cleanup (never a global wipe): other specs share this daemon.
async function cleanup(page: Page, baseURL: string) {
  const projects = (await (
    await page.request.get(`${baseURL}/projects`)
  ).json()) as Array<{
    id: string;
    name: string;
  }>;
  for (const p of projects) {
    if (p.name.startsWith(PREFIX))
      await page.request.delete(`${baseURL}/projects/${p.id}`);
  }
}

test.afterEach(async ({ page, baseURL }) => {
  await cleanup(page, baseURL!);
});

async function box(l: Locator) {
  const b = await l.boundingBox();
  expect(b).not.toBeNull();
  return b!;
}

/** The path, its mark and the loupe, left to right, none over another. */
async function expectSideBySide(page: Page, last: string, badge: boolean) {
  const head = page.getByTestId("target-repo-path-head");
  const tail = page.getByTestId("target-repo-path-tail");
  const loupe = page.getByTestId("repo-browse-trigger");
  await expect(tail).toHaveText(`/${last}`);
  // The folders are cut when they do not fit, the last segment is whole.
  if (badge)
    expect(await head.evaluate((e) => e.scrollWidth > e.clientWidth)).toBe(
      true,
    );
  expect(await tail.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true);
  const t = await box(tail);
  const l = await box(loupe);
  if (badge) {
    const b = await box(page.getByTestId("project-default-badge"));
    expect(t.x + t.width).toBeLessThanOrEqual(b.x + 0.5);
    expect(b.x + b.width).toBeLessThanOrEqual(l.x + 0.5);
  } else {
    expect(t.x + t.width).toBeLessThanOrEqual(l.x + 0.5);
  }
  // The loupe takes the click: nothing lies over it.
  const hit = await page.evaluate(
    ({ x, y }) =>
      document
        .elementFromPoint(x, y)
        ?.closest("[data-testid='repo-browse-trigger']") !== null,
    { x: l.x + l.width / 2, y: l.y + l.height / 2 },
  );
  expect(hit).toBe(true);
}

test("a deep default path beside its mark, and another repository without it", async ({
  page,
  baseURL,
}) => {
  const created = await page.request.post(`${baseURL}/projects`, {
    data: { name: PROJECT_NAME, members: [DEEP, OTHER] },
  });
  expect(created.status()).toBeLessThan(300);
  const { id } = (await created.json()) as { id: string };

  await page.goto("/");
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });
  await page.getByRole("tab", { name: "Runs" }).click();
  await page.getByTestId("new-run-button").click();
  await page.getByTestId("project-combobox").click();
  await page
    .locator(`[data-testid="project-option"][data-project-id="${id}"]`)
    .click();
  await expect(page.getByTestId("target-repo-input")).toHaveValue(DEEP);
  await expect(page.getByTestId("project-default-badge")).toHaveText("default");

  await expectSideBySide(page, LAST, true);
  // The full path on hover.
  await expect(page.getByTestId("target-repo-input")).toHaveAttribute(
    "title",
    DEEP,
  );

  // Another repository: no mark, the path takes its room. The list opens on
  // the chevron (a field that holds a listed repository does not on focus).
  await page.getByTestId("repo-groups-trigger").click();
  await page
    .getByTestId("repo-group-item")
    .filter({ hasText: "shop-api" })
    .click();
  await page.getByTestId("target-repo-input").blur();
  await expect(page.getByTestId("target-repo-input")).toHaveValue(OTHER);
  await expect(page.getByTestId("project-default-badge")).toHaveCount(0);
  await expectSideBySide(page, "shop-api", false);
});
