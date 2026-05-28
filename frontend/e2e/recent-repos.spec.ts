import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-recent-repos-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".maestro", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);
const PROMPTS_DIR = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.prompts`);

const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
    view: { x: 0, y: 100 }
  - id: worker
    name: Worker
    type: doc-only
    prompt_file: ${PIPELINE_NAME}.prompts/worker.md
    inputs:
      - name: task
    outputs:
      - name: result
    view: { x: 200, y: 100 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
    view: { x: 400, y: 100 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
`;

test.beforeAll(async () => {
  process.env.MAESTRO_TMUX_CMD_OVERRIDE =
    'exec sh -c "sleep 300"';
  await fs.mkdir(PROMPTS_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
  await fs.writeFile(path.join(PROMPTS_DIR, "worker.md"), "You are a worker.\n");
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await fs.rm(PROMPTS_DIR, { recursive: true, force: true });
  delete process.env.MAESTRO_TMUX_CMD_OVERRIDE;
});

test("recent repos dropdown appears on focus when runs exist", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  // Seed a run with a target_repo via API
  const resp = await page.request.post(`${baseURL}/runs`, {
    data: {
      pipeline: PIPELINE_NAME,
      input: "seed for recent repos test",
      target_repo: WORKSPACE_ROOT,
    },
  });
  expect(resp.status()).toBe(201);

  // Give the store a moment to refresh
  await page.waitForTimeout(500);

  // Re-navigate so the app fetches recent repos on mount
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  // Open New Run modal
  await page.getByRole("button", { name: "New Run" }).click();
  await expect(page.getByTestId("target-repo-input")).toBeVisible();

  // The input should be pre-filled with the workspace root
  await expect(page.getByTestId("target-repo-input")).toHaveValue(WORKSPACE_ROOT);

  // Validation should trigger automatically (green border or valid message)
  await expect(page.getByTestId("repo-valid")).toBeVisible({ timeout: 10_000 });
});

test("clicking a dropdown item fills the input", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  // Seed runs with two different repos
  const resp1 = await page.request.post(`${baseURL}/runs`, {
    data: {
      pipeline: PIPELINE_NAME,
      input: "seed 1",
      target_repo: WORKSPACE_ROOT,
    },
  });
  expect(resp1.status()).toBe(201);

  // Re-navigate to pick up the store
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  // Open New Run modal
  await page.getByRole("button", { name: "New Run" }).click();
  await expect(page.getByTestId("target-repo-input")).toBeVisible();

  // Clear the input to see all dropdown items
  await page.getByTestId("target-repo-input").clear();
  await page.getByTestId("target-repo-input").focus();

  // Dropdown should appear
  await expect(page.getByTestId("recent-repos-dropdown")).toBeVisible();

  // Click the first item
  const firstItem = page.getByTestId("recent-repo-item").first();
  await firstItem.click();

  // The input should now have the repo path
  await expect(page.getByTestId("target-repo-input")).not.toHaveValue("");

  // Dropdown should be closed
  await expect(page.getByTestId("recent-repos-dropdown")).not.toBeVisible();
});

test("typing non-matching path closes the dropdown", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  // Seed a run
  const resp = await page.request.post(`${baseURL}/runs`, {
    data: {
      pipeline: PIPELINE_NAME,
      input: "seed",
      target_repo: WORKSPACE_ROOT,
    },
  });
  expect(resp.status()).toBe(201);

  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  await page.getByRole("button", { name: "New Run" }).click();
  await expect(page.getByTestId("target-repo-input")).toBeVisible();

  // Clear and type a non-matching path
  await page.getByTestId("target-repo-input").clear();
  await page.getByTestId("target-repo-input").fill("/this/does/not/match/anything");

  // Dropdown should not be visible (no matching items)
  await expect(page.getByTestId("recent-repos-dropdown")).not.toBeVisible();
});
