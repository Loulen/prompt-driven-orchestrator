import { test, expect, type Locator, type Page } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as os from "node:os";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { E2E_TARGET_REPO, runBody } from "./helpers";

// Layer 3b — the lists of the left panel in a real browser: a Run row is two
// lines of 46 px whose long name ends in an ellipsis; its hover bar floats over
// the row without moving a line; the extensions' marks sit at the end of the
// meta line without growing the row; a Trigger switches from its row and a
// Pipeline opens from its row, one gesture each; a two-line Trigger row's hover
// bar sits on its second line, so an extension's button on its first line stays
// clickable.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const BUNDLE_PATH = path.join(WORKSPACE_ROOT, "crates/pdo/tests/test_extension/ui.js");
const PIPELINE_DIR = path.join(os.homedir(), ".pdo", "pipelines");
const PIPELINE = `e2e-left-${process.pid}-${Date.now()}`;
const LONG_NAME = `${"a-very-long-run-name-".repeat(9)}end`.slice(0, 200);

const SEED_YAML = `name: ${PIPELINE}
version: "1.0"
prompt_required: false
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: Worker
    type: agent
    isolated_worktree: false
    inputs:
      - name: task
    outputs:
      - name: result
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
`;

test.beforeAll(async () => {
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(path.join(PIPELINE_DIR, `${PIPELINE}.yaml`), SEED_YAML);
});

test.afterAll(async () => {
  await fs.rm(path.join(PIPELINE_DIR, `${PIPELINE}.yaml`), { force: true });
  await fs.rm(path.join(PIPELINE_DIR, `${PIPELINE}.prompts`), { recursive: true, force: true });
});

const RUN_BADGE = { location: "run/item", id: "badge", label: "Badge", type: "decoration", component: "EntityBadge" };

/** The test extension, marking every Run row (`run/item`) unless told otherwise. */
async function playDecorations(page: Page, contributions: object[] = [RUN_BADGE]) {
  const bundle = await fs.readFile(BUNDLE_PATH, "utf8");
  const extension = {
    id: "marks",
    name: "Marks",
    version: "0.0.0",
    ui: {
      bundle_url: "/ext/marks/ui.js",
      react_major: 19,
      ui_module_major: null,
      locations: [],
      contributions,
      commands: [],
    },
  };
  await page.route("**/ext", (route) => route.fulfill({ json: [extension] }));
  await page.route("**/ext/*/ui.js", (route) => route.fulfill({ body: bundle, contentType: "text/javascript" }));
}

/**
 * The Actor's Appearance, served to this page only (`elements` its deviations), so the
 * shared e2e daemon keeps none: a change made here is kept by the stub alone.
 */
async function stubAppearance(page: Page, elements: Record<string, boolean> | null) {
  let stored: unknown = elements && { version: 1, elements };
  await page.route("**/actors/me/appearance", async (route) => {
    if (route.request().method() === "PUT") stored = route.request().postDataJSON();
    await route.fulfill({ json: { appearance: stored } });
  });
}

async function box(locator: Locator) {
  const b = await locator.boundingBox();
  if (!b) throw new Error("no box");
  return b;
}

test("a Run row is two lines of 46 px, its hover bar floats, the extensions' marks end its meta line", async ({
  page,
  baseURL,
}) => {
  const resp = await page.request.post(`${baseURL}/runs`, {
    data: runBody({ pipeline: PIPELINE, input: "seed", name: LONG_NAME, auto_name: false }),
  });
  expect(resp.status()).toBe(201);
  const { run_id: runId } = (await resp.json()) as { run_id: string };

  await page.setViewportSize({ width: 1280, height: 800 });
  await playDecorations(page);
  await stubAppearance(page, null);
  await page.goto("/");
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });
  const row = page.locator(`[data-run-row="${runId}"]`);
  await expect(row).toBeVisible({ timeout: 10_000 });

  expect((await box(row)).height).toBeCloseTo(46, 0);
  const name = row.getByTestId("run-display-label");
  await expect(name).toHaveAttribute("title", LONG_NAME);
  // The name ends in an ellipsis inside the row.
  expect(await name.evaluate((el) => el.scrollWidth > el.clientWidth)).toBe(true);
  expect((await box(name)).x + (await box(name)).width).toBeLessThanOrEqual((await box(row)).x + (await box(row)).width);

  // The extension's mark, at the end of the meta line, before the duration.
  const mark = row.getByTestId("test-badge-run-item");
  await expect(mark).toBeVisible({ timeout: 10_000 });
  const pipeline = await box(row.getByTestId("run-pipeline-name"));
  const markBox = await box(mark);
  expect(markBox.x).toBeGreaterThan(pipeline.x + pipeline.width - 1);
  expect(markBox.y + markBox.height).toBeLessThanOrEqual((await box(row)).y + 46);
  expect((await box(row)).height).toBeCloseTo(46, 0);

  // Hover: the bar floats over the end of the row; no line moves.
  const before = await box(name);
  await row.hover();
  const bar = row.getByTestId("run-row-actions");
  await expect(bar).toBeVisible();
  expect(await box(name)).toEqual(before);
  expect((await box(row)).height).toBeCloseTo(46, 0);
  const barBox = await box(bar);
  expect(barBox.x + barBox.width).toBeLessThanOrEqual((await box(row)).x + (await box(row)).width);

  await page.request.post(`${baseURL}/runs/${runId}/commands`, { data: { kind: "cleanup_run" } });
});

test("a Run row takes one line once Settings › Appearance hides its whole second line, and works the same", async ({
  page,
  baseURL,
}) => {
  const resp = await page.request.post(`${baseURL}/runs`, {
    data: runBody({ pipeline: PIPELINE, input: "seed", name: "one-line row", auto_name: false }),
  });
  expect(resp.status()).toBe(201);
  const { run_id: runId } = (await resp.json()) as { run_id: string };

  await page.setViewportSize({ width: 1280, height: 800 });
  await playDecorations(page);
  await stubAppearance(page, null);
  await page.goto("/");
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });
  const row = page.locator(`[data-run-row="${runId}"]`);
  await expect(row).toBeVisible({ timeout: 10_000 });
  expect((await box(row)).height).toBeCloseTo(46, 0);

  // The Runs list block: the extension's mark by its name and label, the preview beside.
  await page.getByTestId("open-settings").click();
  await page.getByTestId("settings-category-appearance").click();
  const block = page.getByTestId("appearance-block-runs-list");
  await expect(block.getByRole("switch", { name: "Marks · Badge" })).toBeVisible({ timeout: 10_000 });
  const preview = page.getByTestId("appearance-preview-runs-list");
  await expect(preview.locator("[data-run-row]").first()).toHaveAttribute("data-lines", "2");
  for (const name of ["Pipeline", "Duration", "Marks · Badge"]) await block.getByRole("switch", { name }).click();
  await expect(preview.locator("[data-run-row]").first()).toHaveAttribute("data-lines", "1");
  await page.keyboard.press("Escape");

  // The list: one line, status and name shown; hover bar and selection as before.
  await expect(row).toHaveAttribute("data-lines", "1");
  expect((await box(row)).height).toBeCloseTo(30, 0);
  await expect(row.getByTestId("run-status-dot")).toBeVisible();
  await expect(row.getByTestId("run-display-label")).toHaveText("one-line row");
  await row.hover();
  await expect(row.getByTestId("run-row-actions")).toBeVisible();
  await row.click();
  await expect(row).toHaveAttribute("data-open", "true");

  await page.request.post(`${baseURL}/runs/${runId}/commands`, { data: { kind: "cleanup_run" } });
});

test("a Trigger switches from its row, a Pipeline opens from its row: one gesture each", async ({
  page,
  baseURL,
}) => {
  const created = await page.request.post(`${baseURL}/triggers`, {
    data: { name: `${PIPELINE}-nightly`, pipeline_id: PIPELINE, cron: "0 3 * * *", target_repo: E2E_TARGET_REPO },
  });
  expect(created.status()).toBe(201);
  const { id: triggerId } = (await created.json()) as { id: string };

  await page.setViewportSize({ width: 1280, height: 800 });
  await stubAppearance(page, null);
  await page.goto("/");
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  await page.getByRole("tab", { name: "Triggers" }).click();
  const trigger = page.locator(`[data-trigger-row="${triggerId}"]`);
  expect((await box(trigger)).height).toBeCloseTo(46, 0);
  const toggle = trigger.getByTestId("trigger-toggle");
  await expect(toggle).toHaveAttribute("aria-checked", "true");
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-checked", "false");
  await expect(trigger.getByTestId("trigger-schedule")).toHaveText("off");

  await page.getByRole("tab", { name: "Pipelines" }).click();
  const pipelineRow = page.getByTestId(`library-row-${PIPELINE}`);
  expect((await box(pipelineRow)).height).toBeCloseTo(46, 0);
  await pipelineRow.click();
  await expect(page.getByRole("tab", { name: new RegExp(PIPELINE) }).first()).toBeVisible();

  await page.request.delete(`${baseURL}/triggers/${triggerId}`);
});

test("a Trigger row and a Pipeline row take one line once Settings › Appearance hides their second line, and work the same", async ({
  page,
  baseURL,
}) => {
  const created = await page.request.post(`${baseURL}/triggers`, {
    data: {
      name: `${PIPELINE}-one-line`,
      pipeline_id: PIPELINE,
      cron: "0 3 * * *",
      target_repo: E2E_TARGET_REPO,
      guard_command: "true",
    },
  });
  expect(created.status()).toBe(201);
  const { id: triggerId } = (await created.json()) as { id: string };

  await page.setViewportSize({ width: 1280, height: 800 });
  await stubAppearance(page, null);
  // No extension: none of their marks keeps a second line.
  await page.route("**/ext", (route) => route.fulfill({ json: [] }));
  await page.goto("/");
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  await page.getByTestId("open-settings").click();
  await page.getByTestId("settings-category-appearance").click();
  // Triggers list: hide the next fire, the guard and the pipeline; the preview follows.
  const triggers = page.getByTestId("appearance-block-triggers-list");
  await expect(triggers).toContainText("Always shown: Name, enable switch, last result");
  const triggersPreview = page.getByTestId("appearance-preview-triggers-list");
  await expect(triggersPreview.locator("[data-trigger-row]").first()).toHaveAttribute("data-lines", "2");
  for (const name of ["Pipeline", "Next fire", "Guard"]) await triggers.getByRole("switch", { name }).click();
  await expect(triggersPreview.locator("[data-trigger-row]").first()).toHaveAttribute("data-lines", "1");
  // Pipelines list: hide its whole second line.
  const pipelines = page.getByTestId("appearance-block-pipelines-list");
  for (const name of ["Version", "Draft marker", "Modified"]) await pipelines.getByRole("switch", { name }).click();
  await expect(page.getByTestId("appearance-preview-pipelines-list").locator(".lp-row").first()).toHaveAttribute(
    "data-lines",
    "1",
  );
  await page.keyboard.press("Escape");

  // The Triggers list: one line, the last result beside the switch, which still switches.
  await page.getByRole("tab", { name: "Triggers" }).click();
  const trigger = page.locator(`[data-trigger-row="${triggerId}"]`);
  await expect(trigger).toHaveAttribute("data-lines", "1");
  expect((await box(trigger)).height).toBeCloseTo(30, 0);
  await expect(trigger.getByTestId("trigger-schedule")).toHaveCount(0);
  await expect(trigger.getByTestId("trigger-status-dot")).toBeVisible();
  const toggle = trigger.getByTestId("trigger-toggle");
  await expect(toggle).toHaveAttribute("aria-checked", "true");
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-checked", "false");
  await trigger.hover();
  await expect(trigger.getByTestId("trigger-row-actions")).toBeVisible();

  // The Pipelines list: one line, it opens from its row.
  await page.getByRole("tab", { name: "Pipelines" }).click();
  const pipelineRow = page.getByTestId(`library-row-${PIPELINE}`);
  await expect(pipelineRow).toHaveAttribute("data-lines", "1");
  expect((await box(pipelineRow)).height).toBeCloseTo(30, 0);
  await pipelineRow.click();
  await expect(page.getByRole("tab", { name: new RegExp(PIPELINE) }).first()).toBeVisible();

  await page.request.delete(`${baseURL}/triggers/${triggerId}`);
});

test("a two-line Trigger row's hover bar sits on its second line: the extension's button stays clickable; one line keeps it centred", async ({
  page,
  baseURL,
}) => {
  const created = await page.request.post(`${baseURL}/triggers`, {
    data: { name: `${PIPELINE}-slot`, pipeline_id: PIPELINE, cron: "0 3 * * *", target_repo: E2E_TARGET_REPO },
  });
  expect(created.status()).toBe(201);
  const { id: triggerId } = (await created.json()) as { id: string };

  await page.setViewportSize({ width: 1280, height: 800 });
  await playDecorations(page, [
    { location: "trigger/item", id: "action", label: "Action", type: "decoration", component: "TriggerMark" },
  ]);
  await stubAppearance(page, null);
  await page.goto("/");
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });
  await page.getByRole("tab", { name: "Triggers" }).click();
  const trigger = page.locator(`[data-trigger-row="${triggerId}"]`);
  await expect(trigger).toHaveAttribute("data-lines", "2");
  const action = trigger.getByTestId("test-trigger-action");
  await expect(action).toBeVisible({ timeout: 10_000 });

  // Hovered: the bar shows, under the first line; the button is still the
  // element under its own centre, and a click reaches it, not the row.
  await trigger.hover();
  const bar = trigger.getByTestId("trigger-row-actions");
  await expect(bar).toBeVisible();
  const rowBox = await box(trigger);
  const barBox = await box(bar);
  const actionBox = await box(action);
  expect(barBox.y + barBox.height).toBeLessThanOrEqual(rowBox.y + rowBox.height);
  expect(barBox.y).toBeGreaterThanOrEqual(actionBox.y + actionBox.height);
  const centre = { x: actionBox.x + actionBox.width / 2, y: actionBox.y + actionBox.height / 2 };
  expect(
    await page.evaluate(
      ({ x, y }) => Boolean(document.elementFromPoint(x, y)?.closest('[data-testid="test-trigger-action"]')),
      centre,
    ),
  ).toBe(true);
  await page.mouse.click(centre.x, centre.y);
  await expect(action).toHaveAttribute("data-clicks", "1");
  await expect(bar).toBeVisible();
  await expect(trigger).not.toHaveAttribute("data-open", "true");

  // « Delete trigger » still asks; Cancel keeps the Trigger.
  await trigger.getByTestId("trigger-delete").click();
  const confirm = page.getByTestId("confirm-delete-modal");
  await expect(confirm).toContainText("Delete this trigger?");
  await confirm.getByRole("button", { name: "Cancel" }).click();
  await expect(confirm).toHaveCount(0);
  await expect(trigger).toBeVisible();

  // One line: the bar stays centred on the row.
  await page.unroute("**/actors/me/appearance");
  await stubAppearance(page, { "triggers.list.pipeline": false, "triggers.list.last": false, "triggers.list.next": false });
  await page.reload();
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });
  await page.getByRole("tab", { name: "Triggers" }).click();
  await expect(trigger).toHaveAttribute("data-lines", "1");
  await trigger.hover();
  await expect(bar).toBeVisible();
  const oneLine = await box(trigger);
  const centred = await box(bar);
  expect(centred.y + centred.height / 2).toBeCloseTo(oneLine.y + oneLine.height / 2, 0);

  await page.request.delete(`${baseURL}/triggers/${triggerId}`);
});
