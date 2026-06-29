import { type Page, expect } from "@playwright/test";

/**
 * Open a repo/user pipeline into the edit canvas via the post-refonte
 * UnifiedLeftPanel: switch to the Library tab, then click the entry by name.
 *
 * Replaces the pre-canvas-refonte `[data-testid='edit-toggle']` flow, which no
 * longer exists — pipelines are opened from the Library tab now (#146).
 */
export async function openPipelineForEdit(page: Page, name: string): Promise<void> {
  await page.getByRole("tab", { name: "Library" }).click();
  await page.getByText(name).first().click({ timeout: 5_000 });
}

/**
 * Select a run, select one of its nodes, and reveal the **Run inspector**
 * details pane (Inputs/Outputs sections, Mark complete, the failure banner).
 *
 * Post-refonte the run inspector is split into a Run/Edit tab pair, and for an
 * active run the Run pane auto-expands the live terminal to full height — which
 * hides the details pane below it. So this: clicks the run (by its unique
 * 20-char id prefix, not the shared `YYYYMMDD` date), clicks the node, switches
 * to the Run tab when present, and collapses the terminal if it came up
 * full-size (`terminal-fullsize`) so the details pane is on screen.
 */
export async function openRunNodeDetails(
  page: Page,
  runId: string,
  nodeName: string,
): Promise<void> {
  await page.getByText(runId.slice(0, 20)).first().click({ timeout: 5_000 });
  await page.waitForTimeout(500);
  const node = page.getByText(nodeName, { exact: true }).first();
  await expect(node).toBeVisible({ timeout: 5_000 });
  await node.click();

  const runPane = page.getByTestId("inspector-pane-run");
  const runTab = page.getByTestId("inspector-tab-run");
  if (await runTab.count()) {
    await runTab.click({ timeout: 5_000 });
  }

  // An active run opens with the terminal expanded to full height, which hides
  // the details pane (Inputs/Outputs + Mark complete) below it. Wait for the run
  // pane to render, then — if the details pane isn't present — toggle the
  // terminal's expand button once to reveal it.
  await runPane
    .getByTestId("tmux-terminal")
    .waitFor({ state: "visible", timeout: 5_000 })
    .catch(() => {});
  if ((await runPane.getByTestId("details-pane").count()) === 0) {
    const expand = runPane.getByTestId("term-expand");
    if (await expand.count()) {
      await expand.first().click();
    }
  }
  await expect(runPane.getByTestId("details-pane")).toBeVisible({ timeout: 5_000 });
}

/** baseURL of the e2e daemon, mirroring playwright.config's PDO_E2E_PORT. */
const E2E_BASE_URL = `http://127.0.0.1:${Number(process.env.PDO_E2E_PORT ?? 5273)}`;

/**
 * Archive one or more runs and reap their node + manager tmux sessions
 * (`cleanup_run`).
 *
 * A run whose stub node stays `running` (the e2e `sleep` stub) never reaches a
 * terminal state, so its sessions are not reaped on transition and its node
 * keeps consuming a global admission slot (admission::count_live_node_sessions).
 * Across a full suite that piles up live sessions — eventually throttling later
 * specs into `waiting` and stressing the shared tmux server. Calling this from
 * `afterAll` keeps each spec's footprint near zero.
 *
 * Uses `fetch` (not a Page fixture) so it works in `afterAll`, where no page is
 * available. Best-effort: never throws.
 */
export async function cleanupRuns(
  ...runIds: (string | undefined)[]
): Promise<void> {
  for (const runId of runIds) {
    if (!runId) continue;
    try {
      // Archive: marks the run terminal (frees its admission slot) + reaps its
      // node/manager tmux sessions.
      await fetch(`${E2E_BASE_URL}/runs/${runId}/commands`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ kind: "cleanup_run" }),
      });
      // Forget: drop the archived run from the runs list entirely, so a spec's
      // runs leave no residue that could perturb another spec's run-list
      // selectors. Requires the run to be Archived first (the call above).
      await fetch(`${E2E_BASE_URL}/runs/${runId}`, { method: "DELETE" });
    } catch {
      // best-effort teardown
    }
  }
}
