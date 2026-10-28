import { test, expect, type Locator, type Page } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as os from "node:os";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Layer 3b — the Workbench shell in a real browser: the 36 px top bar and its
// command field, the 22 px status bar, three extensions on `topbar/end` and a
// `statusbar` decoration side by side, and 15 document tabs that scroll, keep
// the active one in view, list themselves in a menu, reorder by drag and close.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
// The instance pipelines the daemon watches.
const PIPELINE_DIR = path.join(os.homedir(), ".pdo", "pipelines");
const BUNDLE_PATH = path.join(WORKSPACE_ROOT, "crates/pdo/tests/test_extension/ui.js");
const TAB_COUNT = 15;
const PREFIX = `e2e-shell-${process.pid}-${Date.now()}`;
const pipelineName = (i: number) => `${PREFIX}-p${String(i).padStart(2, "0")}`;

function seedYaml(name: string): string {
  return `name: ${name}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: alpha
    name: alpha
    type: agent
    isolated_worktree: false
    inputs:
      - name: in
    outputs:
      - name: out
    view: { x: 100, y: 100 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges: []
`;
}

test.beforeAll(async () => {
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  for (let i = 0; i < TAB_COUNT; i++) {
    await fs.writeFile(path.join(PIPELINE_DIR, `${pipelineName(i)}.yaml`), seedYaml(pipelineName(i)));
  }
});

test.afterAll(async () => {
  for (let i = 0; i < TAB_COUNT; i++) {
    await fs.rm(path.join(PIPELINE_DIR, `${pipelineName(i)}.yaml`), { force: true });
    await fs.rm(path.join(PIPELINE_DIR, `${pipelineName(i)}.prompts`), { recursive: true, force: true });
  }
});

/** Three test extensions, one `topbar/end` page each; the first also decorates the status bar. */
async function playThreeExtensions(page: Page) {
  const bundle = await fs.readFile(BUNDLE_PATH, "utf8");
  const extensions = [
    ["one", "Docs"],
    ["two", "Reports"],
    ["three", "Deployments"],
  ].map(([id, label], i) => ({
    id,
    name: label,
    version: "0.0.0",
    ui: {
      bundle_url: `/ext/${id}/ui.js`,
      react_major: 19,
      ui_module_major: null,
      locations: [],
      contributions: [
        { location: "topbar/end", id: "page", label, type: "page", component: "TestPage" },
        ...(i === 0
          ? [{ location: "statusbar", id: "badge", label: "Badge", type: "decoration", component: "EntityBadge" }]
          : []),
      ],
      commands: [],
    },
  }));
  await page.route("**/ext", (route) => route.fulfill({ json: extensions }));
  await page.route("**/ext/*/ui.js", (route) => route.fulfill({ body: bundle, contentType: "text/javascript" }));
}

async function box(locator: Locator) {
  const b = await locator.boundingBox();
  if (!b) throw new Error("no box");
  return b;
}

test("the top bar is 36 px with its command field, the status bar 22 px", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto("/");
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  expect((await box(page.getByTestId("topbar"))).height).toBeCloseTo(36, 0);
  expect((await box(page.getByTestId("statusbar"))).height).toBeCloseTo(22, 0);

  await page.getByTestId("command-field").click();
  await expect(page.getByTestId("command-palette-input")).toBeFocused();
  await page.keyboard.type("anything");
  await expect(page.getByTestId("command-palette-empty")).toHaveText("No match");
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("command-palette")).toBeHidden();
  await page.keyboard.press("Control+k");
  await expect(page.getByTestId("command-palette")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("command-palette")).toBeHidden();
});

test("three extensions on topbar/end line up before the core's icons, each with its tooltip", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await playThreeExtensions(page);
  await page.goto("/");
  const group = page.getByRole("group", { name: "Extensions" });
  await expect(group.getByRole("button")).toHaveCount(3, { timeout: 10_000 });

  const buttons = await group.getByRole("button").all();
  const boxes = await Promise.all(buttons.map(box));
  for (let i = 1; i < boxes.length; i++) {
    expect(boxes[i].y).toBeCloseTo(boxes[0].y, 0);
    expect(boxes[i].x).toBeGreaterThanOrEqual(boxes[i - 1].x + boxes[i - 1].width);
  }
  const newRun = await box(page.getByTestId("topbar-new-run"));
  expect(newRun.x).toBeGreaterThan(boxes[2].x + boxes[2].width);
  expect(newRun.y).toBeCloseTo(boxes[0].y, 0);

  for (const [i, label] of ["Docs", "Reports", "Deployments"].entries()) {
    await buttons[i].hover();
    await expect(page.getByTestId("tooltip-content")).toHaveText(label);
    await page.mouse.move(0, 400);
    await expect(page.getByTestId("tooltip-content")).toBeHidden();
  }

  // The status bar decoration, on the one line of the bar, clear of the version.
  const badge = page.getByTestId("test-badge-statusbar");
  await expect(badge).toBeVisible();
  const bar = await box(page.getByTestId("statusbar"));
  const badgeBox = await box(badge);
  expect(badgeBox.y).toBeGreaterThanOrEqual(bar.y);
  expect(badgeBox.y + badgeBox.height).toBeLessThanOrEqual(bar.y + bar.height + 0.5);
  const version = await box(page.getByTestId("statusbar-version"));
  expect(badgeBox.x + badgeBox.width).toBeLessThanOrEqual(version.x);
});

test("15 tabs scroll, list themselves in a menu, reorder by drag and close", async ({ page }) => {
  test.setTimeout(90_000);
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto("/");
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });
  await page.getByRole("tab", { name: "Pipelines" }).click();
  for (let i = 0; i < TAB_COUNT; i++) {
    await page.getByText(pipelineName(i), { exact: true }).first().click();
    await expect(page.getByTestId(`tab-title-${pipelineName(i)}`)).toBeAttached({ timeout: 5_000 });
  }

  const list = page.getByTestId("tab-list");
  expect(await list.evaluate((el) => el.scrollWidth > el.clientWidth)).toBe(true);
  // The active tab, the last opened, is in view.
  const active = page.getByRole("tab", { selected: true }).filter({ has: page.getByTestId(`tab-title-${pipelineName(14)}`) });
  await expect(active).toBeInViewport();

  // The overflow menu lists every tab and opens the one picked.
  await page.getByTestId("tab-overflow").click();
  const menu = page.getByRole("menu", { name: "Open tabs" });
  await expect(menu.getByRole("menuitemcheckbox")).toHaveCount(TAB_COUNT);
  await menu.getByRole("menuitemcheckbox", { name: `${pipelineName(0)}.yaml` }).click();
  const first = page.getByTestId(`tab-title-${pipelineName(0)}`).locator("xpath=ancestor::button[1]");
  await expect(first).toHaveAttribute("aria-selected", "true");
  await expect(first).toBeInViewport();

  // A mounted page opened by its link, after them all, comes into view too.
  const pageName = `${PREFIX}-guide`;
  await page.route("**/pages", (route) =>
    route.fulfill({
      json: [{ name: pageName, directory: "/tmp", run_id: null, node_id: null, created_at: "2026-10-28T00:00:00Z", guide: null }],
    }),
  );
  await page.evaluate((name) => {
    window.location.hash = `#/pages/${name}`;
  }, pageName);
  const pageTab = page.getByTestId(`page-tab-${pageName}`);
  await expect(pageTab).toHaveAttribute("aria-selected", "true");
  await expect(pageTab).toBeInViewport({ ratio: 1 });
  await page.getByTestId(`page-tab-close-${pageName}`).click();
  await expect(pageTab).toHaveCount(0);

  // Drag the first tab past the third.
  const order = () =>
    page.getByTestId("tab-list").locator('[data-testid^="tab-title-"]').evaluateAll((els) =>
      els.map((el) => el.getAttribute("data-testid")!.replace("tab-title-", "")),
    );
  const third = page.getByTestId(`tab-title-${pipelineName(2)}`).locator("xpath=ancestor::button[1]");
  const thirdBox = await box(third);
  await first.dragTo(third, { targetPosition: { x: thirdBox.width - 4, y: thirdBox.height / 2 } });
  expect((await order()).slice(0, 3)).toEqual([pipelineName(1), pipelineName(2), pipelineName(0)]);

  // Close it from its cross.
  await first.hover();
  await page.getByRole("button", { name: `Close ${pipelineName(0)}.yaml` }).click();
  expect(await order()).toHaveLength(TAB_COUNT - 1);
  expect(await order()).not.toContain(pipelineName(0));
});
