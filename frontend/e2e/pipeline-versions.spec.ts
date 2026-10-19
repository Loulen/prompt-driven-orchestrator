import { test, expect, type Page } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { cleanupRuns, E2E_TARGET_REPO, openPipelineForEdit, pickPipeline } from "./helpers";

// Layer 3b — ADR-0088: Save freezes the shared draft as a version. The history
// lists them, shows one read only and restores it into the draft; a launch
// with a draft that differs from the last version asks first, and the Run
// keeps the version it launched; « Keep old as new pipeline » lists the
// version saved until now as a pipeline of its own.

const PIPELINE_NAME = `e2e-versions-${process.pid}-${Date.now()}`;
let PIPELINE_PATH = "";
const runIds: string[] = [];

const YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - { name: user_prompt }
    view: { x: 0, y: 100 }
  - id: end
    name: End
    type: end
    inputs:
      - { name: result }
    view: { x: 300, y: 100 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: end, port: result }
`;

test.beforeAll(async ({ request }) => {
  const created = await request.post("/pipelines", { data: { name: PIPELINE_NAME } });
  expect(created.ok()).toBe(true);
  PIPELINE_PATH = ((await created.json()) as { path: string }).path;
  await fs.writeFile(PIPELINE_PATH, YAML);
});

test.afterAll(async ({ request }) => {
  await cleanupRuns(...runIds);
  const listed = (await (await request.get("/pipelines")).json()) as { id: string; name: string }[];
  for (const p of listed.filter((p) => p.name.startsWith(PIPELINE_NAME))) {
    await request.delete(`/pipelines/${encodeURIComponent(p.id)}`);
  }
  await fs.rm(path.join(path.dirname(PIPELINE_PATH), `${PIPELINE_NAME}.prompts`), { recursive: true, force: true });
});

async function versionsTab(page: Page) {
  if (!(await page.getByTestId("pipeline-info-panel").isVisible())) {
    await page.getByTestId("toolbar-info").click();
  }
  await page.getByTestId("info-tab-versions").click();
  return page.getByTestId("versions-tab");
}

test("save versions, view and restore one, launch the last, keep the old as a pipeline", async ({ page }) => {
  test.setTimeout(90_000);
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });
  await openPipelineForEdit(page, PIPELINE_NAME);
  const tabTitle = page.getByTestId(`tab-title-${PIPELINE_NAME}`);
  await expect(tabTitle).toHaveText(`${PIPELINE_NAME}.yaml`);

  // 1. Change the draft, Save… offers v2, ticked: save it.
  await page.getByTestId("toolbar-info").click();
  await page.getByTestId("prompt-required-checkbox").uncheck();
  await expect(tabTitle).toHaveText(`• ${PIPELINE_NAME}.yaml`);
  await expect(page.getByTestId("draft-changes")).toContainText("since v1");
  await page.getByTestId("save-button").click();
  const save = page.getByTestId("save-version-modal");
  await expect(save.getByTestId("save-version-new")).toBeChecked();
  await expect(save.getByTestId("save-version-title-input")).toHaveValue("v2");
  await expect(save.getByTestId("save-version-confirm")).toHaveText(/Save v2/);
  await save.getByTestId("save-version-confirm").click();
  await expect(tabTitle).toHaveText(`${PIPELINE_NAME}.yaml`);

  // 2. The history has v2 and v1; v1 opens read only and goes back into the draft.
  const history = await versionsTab(page);
  await expect(history.getByTestId("version-row-2")).toContainText("last");
  await expect(history.getByTestId("version-row-1")).toContainText("1.0");
  await history.getByTestId("version-row-1").hover();
  await history.getByTestId("version-view-1").click();
  await expect(page.getByTestId(`tab-title-__version__${PIPELINE_NAME}@1`)).toHaveText(`${PIPELINE_NAME}.yaml @ v1`);
  await expect(page.getByTestId("version-banner")).toContainText("Viewing v1");
  await page.getByTestId("version-banner-restore").click();
  await expect(tabTitle).toHaveText(`• ${PIPELINE_NAME}.yaml`);
  await page.getByTestId("info-tab-info").click();
  await expect(page.getByTestId("prompt-required-checkbox")).toBeChecked();

  // 3. Launch: the window offers « Save as v3 and run »; « Run last version » runs v2.
  await page.getByRole("tab", { name: "Runs" }).click();
  await page.getByRole("button", { name: "New Run" }).click();
  await page.getByTestId("target-repo-input").fill(E2E_TARGET_REPO);
  await pickPipeline(page, PIPELINE_NAME);
  await page.getByPlaceholder(/free-text prompt/i).fill("e2e versions");
  await page.getByTestId("launch-button").click();
  const launch = page.getByTestId("launch-draft-modal");
  await expect(launch.getByTestId("launch-draft-confirm")).toHaveText(/Save as v3 and run/);
  await launch.getByTestId("launch-draft-last").click();
  const [created] = await Promise.all([
    page.waitForResponse((r) => r.url().endsWith("/runs") && r.request().method() === "POST"),
    launch.getByTestId("launch-draft-confirm").click(),
  ]);
  const runId = ((await created.json()) as { run_id: string }).run_id;
  runIds.push(runId);
  const run = (await (await page.request.get(`/runs/${runId}`)).json()) as {
    pipeline_version?: { number: number };
  };
  expect(run.pipeline_version?.number).toBe(2);
  // The draft stayed as it was: nothing was saved.
  await expect(tabTitle).toHaveText(`• ${PIPELINE_NAME}.yaml`);

  // 4. Save with « Keep old as new pipeline »: « <name> (v2) » is in the library.
  await page.getByTestId(`tab-title-${PIPELINE_NAME}`).click();
  await page.getByTestId("save-button").click();
  await save.getByTestId("save-version-keep-old").check();
  await expect(save.getByTestId("save-version-keep-hint")).toContainText(`${PIPELINE_NAME} (v2)`);
  await save.getByTestId("save-version-confirm").click();
  await page.getByRole("tab", { name: "Pipelines" }).click();
  await expect(page.getByText(`${PIPELINE_NAME} (v2)`, { exact: true })).toBeVisible();
});
