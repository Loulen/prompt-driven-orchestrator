import { test, expect, type Page, type Route } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as os from "node:os";
import * as path from "node:path";
import { cleanupRuns, runMultipart } from "./helpers";

// Layer 3b — archiving a Run from its confirmation, on the kit's
// ConfirmDialog: while the daemon archives, the button spins and the dialog
// stays open, locked (Escape does nothing); it closes once the Run is
// archived. A refusal keeps it open with the daemon's message.
//
// The archive command is held in the browser (`page.route`) until the spec
// has looked at the busy dialog, then let through to the real daemon.

const PIPELINE_NAME = `e2e-archive-confirm-${process.pid}-${Date.now()}`;
// Instance pipelines live under `$HOME/.pdo/pipelines` (ADR-0059).
const PIPELINE_DIR = path.join(os.homedir(), ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);
// Start wired straight to End: the Run completes as soon as it starts.
const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
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
    view: { x: 300, y: 100 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: end, port: result }
`;

const createdRunIds: string[] = [];

test.beforeAll(async () => {
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await cleanupRuns(...createdRunIds);
});

async function runStatus(page: Page, baseURL: string, runId: string): Promise<string> {
  const resp = await page.request.get(`${baseURL}/runs/${runId}`);
  expect(resp.status()).toBe(200);
  return (await resp.json()).status;
}

/** A completed Run, made through the API. */
async function completedRun(page: Page, baseURL: string): Promise<string> {
  const resp = await page.request.post(`${baseURL}/runs`, {
    multipart: runMultipart({ pipeline: PIPELINE_NAME, input: "archive me" }),
  });
  expect(resp.status()).toBe(201);
  const { run_id } = await resp.json();
  createdRunIds.push(run_id);
  await expect.poll(() => runStatus(page, baseURL, run_id), { timeout: 10_000 }).toBe("completed");
  return run_id;
}

/** Open the archive confirmation of `runId` from its row's « ⋯ » menu. */
async function openArchiveConfirm(page: Page, runId: string) {
  const row = page.locator(`[data-run-row="${runId}"]`);
  await expect(row).toBeVisible({ timeout: 10_000 });
  await row.hover();
  await row.getByTestId("run-row-menu").click();
  await page.getByTestId("run-menu-cleanup").click();
  const dialog = page.getByTestId("cleanup-confirm-modal");
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText(runId);
  return dialog;
}

/** The archive command of `runId`, held until the returned function lets it go. */
async function holdArchive(page: Page, runId: string): Promise<() => Promise<void>> {
  let held: Route | undefined;
  let arrived!: () => void;
  const reached = new Promise<void>((res) => (arrived = res));
  await page.route(`**/runs/${runId}/commands`, async (route) => {
    if (route.request().method() !== "POST") return route.continue();
    held = route;
    arrived();
  });
  return async () => {
    await reached;
    await held!.continue();
  };
}

test("archiving a Run waits in its confirmation, locked, then closes", async ({ page, baseURL }) => {
  await page.goto("/");
  await expect(page.getByLabel(/^Daemon: connected/)).toBeVisible({ timeout: 10_000 });
  const runId = await completedRun(page, baseURL!);

  const dialog = await openArchiveConfirm(page, runId);
  const release = await holdArchive(page, runId);
  const confirm = dialog.getByTestId("cleanup-confirm-button");
  await confirm.click();

  // In flight: the button spins with the present tense, Cancel is inert.
  await expect(confirm).toHaveText("Archiving…");
  await expect(confirm).toHaveAttribute("data-pending", "busy");
  await expect(dialog.getByRole("button", { name: "Cancel" })).toHaveAttribute("aria-disabled", "true");

  // Escape, and a click on the backdrop, close nothing while it runs.
  await page.keyboard.press("Escape");
  await page.mouse.click(5, 5);
  await expect(dialog).toBeVisible();
  await expect(confirm).toHaveText("Archiving…");

  await release();
  await expect(dialog).toBeHidden({ timeout: 10_000 });
  await expect.poll(() => runStatus(page, baseURL!, runId), { timeout: 10_000 }).toBe("archived");
});

test("a refused archive keeps the confirmation open with the daemon's message", async ({ page, baseURL }) => {
  await page.goto("/");
  await expect(page.getByLabel(/^Daemon: connected/)).toBeVisible({ timeout: 10_000 });
  const runId = await completedRun(page, baseURL!);

  const dialog = await openArchiveConfirm(page, runId);
  await page.route(`**/runs/${runId}/commands`, (route) =>
    route.request().method() === "POST"
      ? route.fulfill({
          status: 409,
          contentType: "application/json",
          body: JSON.stringify({ error: "worktree is locked by another git process" }),
        })
      : route.continue(),
  );
  await dialog.getByTestId("cleanup-confirm-button").click();

  const banner = dialog.getByRole("alert");
  await expect(banner).toContainText("Archiving failed");
  await expect(banner).toContainText("worktree is locked by another git process");
  await expect(dialog.getByTestId("cleanup-confirm-button")).toHaveText("Retry");
  expect(await runStatus(page, baseURL!, runId)).toBe("completed");

  // Cancel is live again.
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(dialog).toBeHidden();
});
