import { test, expect, type Page } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as os from "node:os";
import * as path from "node:path";
import { execFileSync } from "node:child_process";
import { openPipelineForEdit } from "./helpers";

// Layer 4 (e2e) per ADR 0004 — a Pipeline validated for all Projets and listed
// first for some (CONTEXT.md « Pipeline validé »). The Validate modal keeps the
// Projet boxes live under « All projects »; New Run puts the Pipeline in the
// Projet's section for a listed Projet, in the global one elsewhere. And the
// selected row's check keeps its 13 px clear of the name, which jsdom (no
// layout) cannot pin.

const PREFIX = "e2e-first-";
const STAMP = `${process.pid}-${Date.now()}`;
const PIPELINE_NAME = `${PREFIX}${STAMP}`;
// Instance pipelines live under `$HOME/.pdo/pipelines`, the daemon's instance root.
const PIPELINE_DIR = path.join(os.homedir(), ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);

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

let listedRepo = "";
let otherRepo = "";
const projectIds: string[] = [];

function gitInit(dir: string) {
  execFileSync("git", ["init", "-b", "main"], { cwd: dir });
  execFileSync("git", ["config", "user.email", "t@t.c"], { cwd: dir });
  execFileSync("git", ["config", "user.name", "t"], { cwd: dir });
  execFileSync("git", ["commit", "--allow-empty", "-m", "init"], { cwd: dir });
}

test.beforeAll(async () => {
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
  listedRepo = await fs.mkdtemp(path.join(os.tmpdir(), "pdo-first-listed-"));
  otherRepo = await fs.mkdtemp(path.join(os.tmpdir(), "pdo-first-other-"));
  gitInit(listedRepo);
  gitInit(otherRepo);
});

test.afterAll(async ({ request, baseURL }) => {
  for (const id of projectIds) await request.delete(`${baseURL}/projects/${id}`);
  await fs.rm(PIPELINE_PATH, { force: true });
  for (const dir of [listedRepo, otherRepo]) await fs.rm(dir, { recursive: true, force: true });
});

/** New Run launched in the Projet `projectId` (ADR-0087), on its `repo`. */
async function openNewRunOn(page: Page, projectId: string, repo: string) {
  await page.getByRole("tab", { name: "Runs" }).click();
  await page.getByRole("button", { name: "New Run" }).click();
  await page.getByTestId("project-combobox").click();
  await page.locator(`[data-testid="project-option"][data-project-id="${projectId}"]`).click();
  await page.getByTestId("target-repo-input").fill(repo);
  await expect(page.getByTestId("pipeline-select")).toBeEnabled({ timeout: 10_000 });
  await page.getByTestId("pipeline-select").click();
  await expect(page.getByTestId("pipeline-select-menu")).toBeVisible();
}

async function closeNewRun(page: Page) {
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("pipeline-select-menu")).toBeHidden();
  await page.getByRole("button", { name: "Close" }).click();
  await expect(page.getByTestId("target-repo-input")).toBeHidden();
}

test("a pipeline validated for all heads the menu of the Projets it is listed first for", async ({
  page,
  baseURL,
}) => {
  for (const [name, repo] of [
    [`${PREFIX}listed-${STAMP}`, listedRepo],
    [`${PREFIX}other-${STAMP}`, otherRepo],
  ]) {
    const res = await page.request.post(`${baseURL}/projects`, { data: { name, members: [repo] } });
    expect(res.ok()).toBeTruthy();
    projectIds.push(((await res.json()) as { id: string }).id);
  }
  const [listedId, otherId] = projectIds;

  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  // 1. Validate: « All projects », then tick the listed Projet under it.
  await openPipelineForEdit(page, PIPELINE_NAME);
  await page.getByTestId("validate-button").click();
  const modal = page.getByTestId("validate-pipeline-modal");
  await expect(modal).toBeVisible();
  if (!(await page.getByTestId("validate-all").isChecked())) {
    await page.getByTestId("validate-all").check();
  }
  await expect(page.getByTestId("validate-projects-heading")).toContainText(/listed first for/i);
  await expect(page.getByTestId(`validate-project-${listedId}`)).toBeEnabled();
  await page.getByTestId(`validate-project-${listedId}`).check();
  await expect(page.getByTestId("validate-verdict")).toContainText(
    `listed first for ${PREFIX}listed-${STAMP}`,
  );
  await page.getByTestId("validate-save").click();
  await expect(modal).toBeHidden();

  const list = (await (await page.request.get(`${baseURL}/pipelines`)).json()) as Array<{
    id: string;
    validation: unknown;
  }>;
  expect(list.find((p) => p.id === PIPELINE_NAME)?.validation).toEqual({
    kind: "all",
    project_ids: [listedId],
  });

  // 2. New Run on the listed Projet's repository: in « Project pipelines ».
  await openNewRunOn(page, listedId, listedRepo);
  const projectSection = page.getByTestId("pipeline-select-section-project");
  await expect(projectSection).toContainText(`Project pipelines · ${PREFIX}listed-${STAMP}`);
  const row = projectSection.getByTestId(`pipeline-select-option-${PIPELINE_NAME}`);
  await expect(row).toBeVisible();
  await expect(
    page.getByTestId("pipeline-select-section-global").getByTestId(`pipeline-select-option-${PIPELINE_NAME}`),
  ).toHaveCount(0);

  // 3. The selected row: no scope column, and the check clear of the name.
  await row.click();
  await page.getByTestId("pipeline-select").click();
  const selected = page.getByTestId(`pipeline-select-option-${PIPELINE_NAME}`);
  await expect(selected).toHaveText(PIPELINE_NAME);
  const check = await selected.locator("svg").boundingBox();
  const name = await selected.locator("span.truncate").boundingBox();
  expect(check).not.toBeNull();
  expect(name).not.toBeNull();
  expect(Math.round(check!.width)).toBe(13);
  expect(check!.x + check!.width).toBeLessThanOrEqual(name!.x);
  await closeNewRun(page);

  // 4. New Run on another Projet's repository: in « Global pipelines ».
  await openNewRunOn(page, otherId, otherRepo);
  await expect(page.getByTestId("pipeline-select-section-project")).toContainText(
    `${PREFIX}other-${STAMP}`,
  );
  await expect(
    page.getByTestId("pipeline-select-section-global").getByTestId(`pipeline-select-option-${PIPELINE_NAME}`),
  ).toBeVisible();
  await closeNewRun(page);

  // Deleting the listed Projet leaves the pipeline validated for all.
  await page.request.delete(`${baseURL}/projects/${listedId}`);
  projectIds.splice(projectIds.indexOf(listedId), 1);
  await expect
    .poll(async () => {
      const after = (await (await page.request.get(`${baseURL}/pipelines`)).json()) as Array<{
        id: string;
        validation: unknown;
      }>;
      return after.find((p) => p.id === PIPELINE_NAME)?.validation;
    })
    .toEqual({ kind: "all" });
});
