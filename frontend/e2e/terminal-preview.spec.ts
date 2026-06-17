import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Layer 3b — Inline xterm.js terminal (refs #55, ADR 0004).
// Verifies:
// 1. Selecting a running node renders the <TmuxTerminal> component.
// 2. The terminal connects via WebSocket and shows live status.
// 3. Typing into the terminal echoes back through the PTY bridge.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-inline-terminal-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);

const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: worker
    type: doc-only
    inputs:
      - name: in
    outputs:
      - name: out
    view: { x: 100, y: 100 }
edges: []
`;

test.beforeAll(async () => {
  process.env.PDO_TMUX_CMD_OVERRIDE =
    'exec sh -c "cat"';
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  delete process.env.PDO_TMUX_CMD_OVERRIDE;
});

test("selecting a running node shows inline xterm terminal", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  const resp = await page.request.post(`${baseURL}/runs`, {
    data: {
      pipeline: PIPELINE_NAME,
      input: "e2e inline terminal test",
    },
  });
  expect(resp.status()).toBe(201);
  const { run_id } = await resp.json();

  await page.getByText(run_id.slice(0, 8)).first().click({ timeout: 5_000 });
  await page.waitForTimeout(500);
  const workerNode = page.getByText("worker", { exact: true }).first();
  await expect(workerNode).toBeVisible({ timeout: 3_000 });
  await workerNode.click();

  const terminal = page.getByTestId("tmux-terminal");
  await expect(terminal).toBeVisible({ timeout: 5_000 });

  const xtermContainer = page.getByTestId("xterm-container");
  await expect(xtermContainer.locator("canvas").first()).toBeVisible({
    timeout: 5_000,
  });

  await expect(
    terminal.locator("text=/attached|connected/"),
  ).toBeVisible({ timeout: 5_000 });

  // Type into the terminal — `cat` echoes back through the PTY bridge
  await xtermContainer.click();
  await page.keyboard.type("echo hello\n", { delay: 50 });

  await expect(async () => {
    const text = await page.evaluate(() => {
      const rows = document.querySelector(
        '[data-testid="xterm-container"] .xterm-rows',
      );
      return rows?.textContent ?? "";
    });
    expect(text).toContain("hello");
  }).toPass({ timeout: 5_000 });

  const sessionName = `pdo-${run_id}-worker-iter-1`;
  const { execSync } = await import("node:child_process");
  try {
    execSync(`tmux kill-session -t ${sessionName}`, { stdio: "ignore" });
  } catch {
    // session may already be dead
  }
});

test("terminal toolbar shows expand and detach buttons", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  const resp = await page.request.post(`${baseURL}/runs`, {
    data: {
      pipeline: PIPELINE_NAME,
      input: "e2e toolbar test",
    },
  });
  expect(resp.status()).toBe(201);
  const { run_id } = await resp.json();

  await page.getByText(run_id.slice(0, 8)).first().click({ timeout: 5_000 });
  await page.waitForTimeout(500);
  const workerNode = page.getByText("worker", { exact: true }).first();
  await expect(workerNode).toBeVisible({ timeout: 3_000 });
  await workerNode.click();

  const terminal = page.getByTestId("tmux-terminal");
  await expect(terminal).toBeVisible({ timeout: 5_000 });

  await expect(page.getByTestId("term-expand")).toBeVisible();
  await expect(page.getByTestId("term-detach")).toBeVisible();

  const sessionName = `pdo-${run_id}-worker-iter-1`;
  const { execSync } = await import("node:child_process");
  try {
    execSync(`tmux kill-session -t ${sessionName}`, { stdio: "ignore" });
  } catch {
    // ok
  }
});
