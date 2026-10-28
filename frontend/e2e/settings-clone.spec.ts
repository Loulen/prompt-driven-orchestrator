import { test, expect, type Page } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as os from "node:os";
import * as path from "node:path";
import { execFileSync } from "node:child_process";

// Layer 4 (e2e) per ADR 0004 — CORE-97: a repository added by its URL, in a real
// browser over a real daemon. Set the clones folder, clone a `file://` repository
// into it from Settings › Projects (it joins the Projet, checked on disk), then New Run
// on another URL clones and launches; the same URL again reuses its clone.

const PREFIX = "e2e-settings-clone-";
const PROJECT_NAME = `${PREFIX}${process.pid}`;
const ROOT = path.join(os.tmpdir(), `${PREFIX}${process.pid}-${Date.now()}`);
const CLONES = path.join(ROOT, "clones");
const PIPELINE_NAME = `e2e-clone-${process.pid}-${Date.now()}`;
// An instance pipeline: a Projet can head its menu with it.
const PIPELINE_PATH = path.join(os.homedir(), ".pdo", "pipelines", `${PIPELINE_NAME}.yaml`);

// Start → End only: nothing spawns, the run completes on its own.
const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
prompt_required: false
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - { name: user_prompt, side: bottom }
    view: { x: 100, y: 0 }
  - id: end
    name: End
    type: end
    inputs:
      - { name: result, side: top }
    view: { x: 100, y: 200 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: end, port: result }
`;

function git(dir: string, args: string[]): string {
  return execFileSync("git", args, { cwd: dir }).toString().trim();
}

/** A repository with two commits at `ROOT/origins/<name>`; its `file://` URL. */
async function origin(name: string): Promise<string> {
  const dir = path.join(ROOT, "origins", name);
  await fs.mkdir(dir, { recursive: true });
  git(dir, ["init", "-b", "main"]);
  git(dir, ["config", "user.email", "t@t.c"]);
  git(dir, ["config", "user.name", "t"]);
  git(dir, ["commit", "--allow-empty", "-m", "one"]);
  git(dir, ["commit", "--allow-empty", "-m", "two"]);
  return `file://${dir}`;
}

let analytics = "";
let shopWeb = "";

test.beforeAll(async () => {
  analytics = await origin("analytics");
  shopWeb = await origin("shop-web");
  await fs.mkdir(path.dirname(PIPELINE_PATH), { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async ({ request, baseURL }) => {
  await request.put(`${baseURL}/settings`, { data: { clones_folder: "" } });
  const projects = (await (await request.get(`${baseURL}/projects`)).json()) as Array<{ id: string; name: string }>;
  for (const p of projects) {
    if (p.name.startsWith(PREFIX)) await request.delete(`${baseURL}/projects/${p.id}`);
  }
  await fs.rm(PIPELINE_PATH, { force: true });
  await fs.rm(ROOT, { recursive: true, force: true });
});

async function openProjectsSettings(page: Page) {
  await page.goto("/");
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });
  await page.getByTestId("open-settings").click();
  await page.getByTestId("settings-category-projects").click();
  await expect(page.getByTestId("projects-panel")).toBeVisible();
}

test("clone a URL into the clones folder from Settings, then New Run clones and launches on another", async ({
  page,
}) => {
  await openProjectsSettings(page);

  // The clones folder, saved with the form.
  await page.getByTestId("settings-section-clones-folder").click();
  await expect(page.getByTestId("setting-source-clones-folder")).toHaveText(
    "Source: built-in default (~/pdo/repos).",
  );
  await page.getByTestId("setting-clones-folder").fill(CLONES);
  await page.getByTestId("settings-save").click();
  await expect(page.getByTestId("setting-source-clones-folder")).toHaveText("Source: stored setting.");

  // A Projet, then a repository cloned by its URL.
  await page.getByTestId("projects-new").click();
  await page.getByTestId("projects-new-name").fill(PROJECT_NAME);
  await page.getByTestId("projects-new-name").press("Enter");
  await expect(page.getByTestId("project-detail-name")).toHaveText(PROJECT_NAME);
  await page.getByTestId("project-add-repo").click();
  await page.getByTestId("project-add-repo-clone").click();
  await page.getByTestId("project-clone-url").fill(analytics);
  const cloned = path.join(CLONES, "analytics");
  await expect(page.getByTestId("project-clone-into")).toHaveText(cloned);
  await page.getByTestId("project-clone-start").click();
  const row = page.locator(`[data-testid="project-repo-row"][data-path="${cloned}"]`);
  await expect(row).toBeVisible({ timeout: 15_000 });
  await expect(row.getByTestId("project-repo-remote")).toHaveText(analytics);
  expect(git(cloned, ["rev-list", "--count", "HEAD"])).toBe("2");

  // The same URL again: the clone is reused.
  await page.getByTestId("project-add-repo").click();
  await page.getByTestId("project-add-repo-clone").click();
  await page.getByTestId("project-clone-url").fill(analytics);
  await expect(page.getByTestId("project-clone-reused")).toBeVisible();
  await page.getByTestId("project-clone-dismiss").click();

  // New Run in the Projet on another URL: clone, then launch on the clone with the
  // Projet's pipeline, which the form preselects.
  const projects = (await (await page.request.get("/projects")).json()) as Array<{ id: string; name: string }>;
  const projectId = projects.find((p) => p.name === PROJECT_NAME)!.id;
  const validated = await page.request.put(`/pipelines/${PIPELINE_NAME}/validation`, {
    data: { kind: "all", project_ids: [projectId] },
  });
  expect(validated.ok(), await validated.text()).toBeTruthy();
  await page.getByRole("button", { name: "Close settings" }).click();
  await page.getByRole("tab", { name: "Runs" }).click();
  await page.getByTestId("new-run-button").click();
  await page.getByTestId("project-combobox").click();
  await page.locator(`[data-testid="project-option"][data-project-id="${projectId}"]`).click();
  await page.getByTestId("target-repo-input").fill(shopWeb);
  const shopClone = path.join(CLONES, "shop-web");
  await expect(page.getByTestId("remote-repo-hint")).toContainText(`will be cloned in full to ${shopClone}`);
  await expect(page.getByTestId("pipeline-select")).toContainText("Available once the clone is done");
  await expect(page.getByTestId("clone-launch-button")).toHaveText("Clone and launch");
  await page.getByPlaceholder(/free-text prompt/i).fill("e2e clone and launch");
  const created = page.waitForResponse(
    (r) => new URL(r.url()).pathname === "/runs" && r.request().method() === "POST",
    { timeout: 20_000 },
  );
  await page.getByTestId("clone-launch-button").click();
  const response = await created;
  expect(response.ok()).toBeTruthy();
  expect(git(shopClone, ["rev-list", "--count", "HEAD"])).toBe("2");
  const run = (await response.json()) as { run_id: string };
  const detail = await (await page.request.get(`/runs/${run.run_id}`)).json();
  expect(JSON.stringify(detail)).toContain(shopClone);
  const project = (await (await page.request.get(`/projects/${projectId}`)).json()) as { members: string[] };
  expect(project.members).toContain(shopClone);
});
