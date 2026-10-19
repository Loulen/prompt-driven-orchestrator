import { test, expect, type Browser, type Page } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { openPipelineForEdit } from "./helpers";

// Layer 3b — ADR-0088: a pipeline of the registry is edited in a shared draft.
// Two browsers on the same pipeline see each other's changes, selection and
// typing without reloading; Undo takes back my own change only; a file written
// outside PDO lands in the draft beside the unsaved changes, with no conflict
// window; the draft survives a reload.

const PIPELINE_NAME = `e2e-draft-${process.pid}-${Date.now()}`;
// The instance registry the daemon reads, wherever its home is: created
// through the API, then written on disk as an editor outside PDO would.
let PIPELINE_PATH = "";
let PROMPTS_DIR = "";

function yaml(name: string, extraNode = ""): string {
  return `name: ${name}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - { name: user_prompt }
    view: { x: 0, y: 100 }
  - id: alpha
    name: alpha
    type: agent
    isolated_worktree: false
    inputs:
      - { name: in }
    outputs:
      - { name: out }
    view: { x: 220, y: 100 }
  - id: gamma
    name: gamma
    type: agent
    isolated_worktree: false
    inputs:
      - { name: in }
    outputs:
      - { name: out }
    view: { x: 220, y: 260 }
${extraNode}  - id: end
    name: End
    type: end
    inputs:
      - { name: result }
    view: { x: 480, y: 100 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: alpha, port: in }
  - source: { node: alpha, port: out }
    target: { node: end, port: result }
`;
}

const BETA = `  - id: beta
    name: beta
    type: agent
    isolated_worktree: false
    view: { x: 480, y: 260 }
`;

async function second(browser: Browser): Promise<Page> {
  const baseURL = test.info().project.use.baseURL!;
  const context = await browser.newContext({
    baseURL,
    storageState: { cookies: [], origins: [{ origin: baseURL, localStorage: [{ name: "pdo.tour.offered", value: "1" }] }] },
  });
  return context.newPage();
}

async function open(page: Page): Promise<void> {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });
  await openPipelineForEdit(page, PIPELINE_NAME);
  await expect(page.getByTestId(`tab-title-${PIPELINE_NAME}`)).toBeVisible();
}

function node(page: Page, name: string) {
  return page.locator(".react-flow__node").filter({ hasText: name }).first();
}

async function promptOf(page: Page, name: string) {
  await node(page, name).click();
  const prompt = page.getByPlaceholder("Enter the node's role prompt...");
  await expect(prompt).toBeVisible();
  return prompt;
}

test.beforeAll(async ({ request }) => {
  const created = await request.post("/pipelines", { data: { name: PIPELINE_NAME } });
  expect(created.ok()).toBe(true);
  PIPELINE_PATH = ((await created.json()) as { path: string }).path;
  PROMPTS_DIR = path.join(path.dirname(PIPELINE_PATH), `${PIPELINE_NAME}.prompts`);
});

test.beforeEach(async () => {
  await fs.writeFile(PIPELINE_PATH, yaml(PIPELINE_NAME));
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await fs.rm(PROMPTS_DIR, { recursive: true, force: true });
});

test("two browsers share the draft, its presence, and their own undo", async ({ page, browser }) => {
  // Two browsers on one daemon, and a reload.
  test.setTimeout(90_000);
  await open(page);
  const other = await second(browser);
  await open(other);

  // Each sees the other.
  await expect(page.getByTestId("draft-presence").getByTestId("draft-participant")).toHaveCount(1);
  await expect(other.getByTestId("draft-presence").getByTestId("draft-participant")).toHaveCount(1);

  // A types a prompt: B sees the text, and who types, without reloading.
  const mine = await promptOf(page, "alpha");
  await mine.click();
  await mine.pressSequentially("Write the spec.");
  const theirs = await promptOf(other, "alpha");
  await expect(theirs).toHaveValue("Write the spec.");
  await expect(other.getByTestId("prompt-typing")).toBeVisible();

  // A selects gamma: B sees whose selection it is.
  await node(page, "gamma").click();
  await expect(node(other, "gamma").getByTestId("selected-by")).toBeVisible();

  // Both change a different field of gamma at once: both stay.
  await node(other, "gamma").click();
  await page.getByTestId("toolbar-add").click();
  await page.getByTestId("add-menu-node").click();
  const box = await node(other, "gamma").boundingBox();
  await other.mouse.move(box!.x + box!.width / 2, box!.y + box!.height / 2);
  await other.mouse.down();
  await other.mouse.move(box!.x + box!.width / 2 + 120, box!.y + box!.height / 2 + 40, { steps: 8 });
  await other.mouse.up();
  await expect(page.locator(".react-flow__node")).toHaveCount(5);
  await expect(other.locator(".react-flow__node")).toHaveCount(5);
  await expect(page.getByTestId(`tab-title-${PIPELINE_NAME}`)).toHaveText(`• ${PIPELINE_NAME}.yaml`);
  await expect(page.getByTestId("draft-changes")).toBeVisible();

  // A undoes: only A's added node goes, B's move stays.
  const moved = await node(page, "gamma").boundingBox();
  await page.getByTestId("toolbar-undo").click();
  await expect(page.locator(".react-flow__node")).toHaveCount(4);
  await expect(other.locator(".react-flow__node")).toHaveCount(4);
  expect(await node(page, "gamma").boundingBox()).toEqual(moved);

  // B leaves: A sees it.
  await other.context().close();
  await expect(page.getByTestId("draft-presence").getByTestId("draft-participant")).toHaveCount(0);

  // The draft survives a reload.
  await page.reload();
  await openPipelineForEdit(page, PIPELINE_NAME);
  await expect(await promptOf(page, "alpha")).toHaveValue("Write the spec.");
  await expect(page.getByTestId(`tab-title-${PIPELINE_NAME}`)).toHaveText(`• ${PIPELINE_NAME}.yaml`);

  // Save: nothing left to save.
  await page.getByTestId("save-button").click();
  await expect(page.getByTestId(`tab-title-${PIPELINE_NAME}`)).toHaveText(`${PIPELINE_NAME}.yaml`);
});

test("a file written outside PDO lands in the draft beside its changes, with no conflict window", async ({ page }) => {
  await open(page);
  const prompt = await promptOf(page, "alpha");
  await prompt.fill("MY_LOCAL_EDIT");
  const title = page.getByTestId(`tab-title-${PIPELINE_NAME}`);
  await expect(title).toHaveText(`• ${PIPELINE_NAME}.yaml`);

  // The watcher can coalesce a write under load: write again until it lands.
  for (let attempt = 0; attempt < 3; attempt++) {
    await fs.writeFile(PIPELINE_PATH, yaml(PIPELINE_NAME, BETA));
    const landed = await node(page, "beta")
      .waitFor({ state: "visible", timeout: 4_000 })
      .then(() => true)
      .catch(() => false);
    if (landed) break;
  }
  await expect(node(page, "beta")).toBeVisible();
  await expect(page.getByText("External edit conflict")).toHaveCount(0);
  await expect(prompt).toHaveValue("MY_LOCAL_EDIT");
  await expect(title).toHaveText(`• ${PIPELINE_NAME}.yaml`);
});
