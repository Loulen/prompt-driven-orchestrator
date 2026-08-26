import { test, expect } from "@playwright/test";
import type { Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { cleanupRuns, runMultipart } from "./helpers";

// FP validation — #598 / ADR-0049: an interactive node whose tmux session dies
// parks `interrupted` (recoverable) and the panel must now offer a way out:
// Play + Reopen banner + Mark complete + a minimized (no "can't find session")
// terminal; a Reopen revives the session; and an archived run shows none of it.
//
// This drives the REAL system end to end: a real daemon (webServer), a real
// tmux session per node, killed for real, and the real stale-detector sweep
// producing `interrupted`. Screenshots are annotated and written to the PDO
// feature-screens artifact directory for the run report.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PORT = Number(process.env.PDO_E2E_PORT ?? 5273);
const SOCKET = `pdo-${PORT}`;
const SHOTS = process.env.PDO_FP_SHOTS_DIR ?? path.join(WORKSPACE_ROOT, "fp-shots");

const PIPELINE_NAME = `e2e-interrupted-fp-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);

// A single worker node with a live session. The stale detector only probes
// nodes in `Running` state (`running_nodes`), so this worker must be running
// when its session dies — the reproduce doc's "running" branch. The panel's
// recovery affordances key on `status === "interrupted"` alone (not node type),
// so a running worker turned interrupted exercises the exact fixed code path.
const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - { name: user_prompt, side: bottom }
    view: { x: 100, y: 0 }
  - id: worker
    name: worker
    type: doc-only
    inputs:
      - { name: task, side: top }
    outputs:
      - { name: summary, side: bottom }
    view: { x: 100, y: 150 }
  - id: end
    name: End
    type: end
    inputs:
      - { name: result, side: top }
    view: { x: 100, y: 300 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: summary }
    target: { node: end, port: result }
`;

const createdRunIds: string[] = [];

// Wide viewport so the right-hand Run inspector (where every affordance lives)
// is fully on screen and the annotation callouts aren't clipped at the edge.
test.use({ viewport: { width: 1600, height: 900 } });

test.beforeAll(async () => {
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
  await fs.mkdir(SHOTS, { recursive: true });
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await cleanupRuns(...createdRunIds);
});

async function waitWorkerStatus(
  page: Page,
  baseURL: string,
  runId: string,
  status: string,
  timeout = 45_000,
) {
  await expect(async () => {
    const resp = await page.request.get(`${baseURL}/runs/${runId}`);
    expect(resp.status()).toBe(200);
    const json = await resp.json();
    expect(json.nodes?.worker?.status).toBe(status);
  }).toPass({ timeout });
}

// An interactive node parks `awaiting_user` (a live, attachable tmux session)
// rather than `running` — that live session is precisely what the stale
// detector later finds dead and turns into `interrupted`.
async function waitWorkerLive(page: Page, baseURL: string, runId: string) {
  await expect(async () => {
    const resp = await page.request.get(`${baseURL}/runs/${runId}`);
    expect(resp.status()).toBe(200);
    const json = await resp.json();
    expect(["running", "awaiting_user"]).toContain(json.nodes?.worker?.status);
  }).toPass({ timeout: 45_000 });
}

async function createRun(page: Page, baseURL: string): Promise<string> {
  const resp = await page.request.post(`${baseURL}/runs`, {
    multipart: runMultipart({ pipeline: PIPELINE_NAME, input: "e2e interrupted-node FP" }),
  });
  expect(resp.status()).toBe(201);
  const { run_id } = await resp.json();
  createdRunIds.push(run_id);
  await waitWorkerLive(page, baseURL, run_id);
  return run_id;
}

// Select a run in the left panel, its worker node, and switch to the Run tab.
async function openWorker(page: Page, runId: string) {
  await page
    .getByText(runId.slice(0, 20))
    .first()
    .click({ timeout: 5_000, position: { x: 5, y: 5 } });
  await page.waitForTimeout(400);
  const node = page.getByText("worker", { exact: true }).first();
  await expect(node).toBeVisible({ timeout: 5_000 });
  await node.click();
  const runTab = page.getByTestId("inspector-tab-run");
  if (await runTab.count()) await runTab.click({ timeout: 5_000 });
}

// Draw a caption banner + numbered callouts over the live page, then screenshot.
// Annotations are pure DOM overlays removed right after the capture.
async function annotateAndShoot(
  page: Page,
  file: string,
  title: string,
  callouts: { testid: string; label: string }[],
) {
  await page.evaluate(
    ({ title, callouts }) => {
      const root = document.createElement("div");
      root.id = "__fp_annot__";
      const cap = document.createElement("div");
      cap.textContent = title;
      Object.assign(cap.style, {
        position: "fixed", top: "0", left: "0", right: "0", zIndex: "2147483647",
        background: "#0b1021", color: "#fff", font: "600 14px/1.5 system-ui, sans-serif",
        padding: "8px 14px", borderBottom: "2px solid #f59e0b", whiteSpace: "pre-wrap",
      } as CSSStyleDeclaration);
      root.appendChild(cap);
      callouts.forEach((c, i) => {
        const el = document.querySelector(`[data-testid="${c.testid}"]`) as HTMLElement | null;
        if (!el) return;
        const r = el.getBoundingClientRect();
        const box = document.createElement("div");
        Object.assign(box.style, {
          position: "fixed", left: `${r.left - 3}px`, top: `${r.top - 3}px`,
          width: `${r.width + 6}px`, height: `${r.height + 6}px`, zIndex: "2147483646",
          border: "2px solid #f59e0b", borderRadius: "6px", boxShadow: "0 0 0 2px rgba(0,0,0,.35)",
          pointerEvents: "none",
        } as CSSStyleDeclaration);
        const tag = document.createElement("div");
        tag.textContent = `${i + 1}. ${c.label}`;
        // Flip the label to open leftward when the element sits in the right
        // third of the viewport, so the caption never runs off-screen.
        const flip = r.left > window.innerWidth * 0.6;
        Object.assign(tag.style, {
          position: "fixed", top: `${r.top - 24}px`, zIndex: "2147483647",
          background: "#f59e0b", color: "#0b1021", font: "700 11px/1.4 system-ui, sans-serif",
          padding: "1px 6px", borderRadius: "4px", whiteSpace: "nowrap",
          ...(flip
            ? { right: `${Math.max(4, window.innerWidth - r.right - 3)}px` }
            : { left: `${r.left - 3}px` }),
        } as CSSStyleDeclaration);
        root.appendChild(box);
        root.appendChild(tag);
      });
      document.body.appendChild(root);
    },
    { title, callouts },
  );
  await page.waitForTimeout(150);
  await page.screenshot({ path: path.join(SHOTS, file), fullPage: false });
  await page.evaluate(() => document.getElementById("__fp_annot__")?.remove());
}

test("interrupted node offers Play + Reopen + Mark complete, revives on Reopen, and hides all of it when archived", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(150_000);
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 15_000 });

  // Two runs: one to exercise the recovery affordances (+ Reopen), one to leave
  // archived-while-interrupted for the archived variant. Kill both sessions,
  // then a single stale-detector sweep turns both `interrupted`.
  const recoverRun = await createRun(page, baseURL!);
  const archiveRun = await createRun(page, baseURL!);

  for (const runId of [recoverRun, archiveRun]) {
    execFileSync("tmux", ["-L", SOCKET, "kill-session", "-t", `pdo-${runId}-worker-iter-1`]);
  }

  // The background sweep (30s cadence) detects the dead sessions → interrupted.
  await waitWorkerStatus(page, baseURL!, recoverRun, "interrupted");
  await waitWorkerStatus(page, baseURL!, archiveRun, "interrupted");

  // ---- Step 1: the interrupted panel shows every recovery affordance --------
  await openWorker(page, recoverRun);
  const banner = page.getByTestId("interrupted-banner");
  await expect(banner).toBeVisible({ timeout: 8_000 });
  await expect(banner).toContainText("Session died");
  await expect(banner).toContainText("the work is presumed intact");
  await expect(page.getByTestId("interrupted-reopen-btn")).toBeVisible();
  await expect(page.getByTestId("play-retry-btn")).toBeVisible();
  await expect(page.getByTestId("play-retry-btn")).toContainText("Play");
  await expect(page.getByTestId("mark-complete-btn")).toBeVisible();
  await expect(page.getByTestId("terminal-minimized")).toBeVisible();
  await expect(page.getByTestId("tmux-terminal")).toHaveCount(0);
  await expect(page.getByText(/can't find session/i)).toHaveCount(0);

  await annotateAndShoot(page, "1-interrupted-affordances.png", [
    "Etape 1 — Node Interrupted : session tmux tuee, detectee par la sweep stale-detector (meme etat NodeInterrupted qu'au redemarrage daemon).",
    "Le volet offre desormais une sortie : banniere Reopen, bouton Play, Mark complete ; terminal minimise (plus de \"can't find session\").",
  ].join("\n"), [
    { testid: "interrupted-banner", label: "Banniere: Session died - the work is presumed intact" },
    { testid: "interrupted-reopen-btn", label: "Bouton Reopen (geste retry)" },
    { testid: "play-retry-btn", label: "Bouton Play dans node-controls" },
    { testid: "mark-complete-btn", label: "Mark complete de nouveau atteignable" },
    { testid: "terminal-minimized", label: "Terminal minimise (session definitivement morte)" },
  ]);

  // ---- Step 2: Reopen revives the session, terminal flips back to split -----
  await page.getByTestId("interrupted-reopen-btn").click();
  await waitWorkerLive(page, baseURL!, recoverRun);
  await expect(page.getByTestId("tmux-terminal")).toBeVisible({ timeout: 10_000 });
  await expect(page.getByTestId("interrupted-banner")).toHaveCount(0);

  await annotateAndShoot(page, "2-reopened-live.png", [
    "Etape 2 — Apres clic sur Reopen : la node se relance (running), la session tmux ranime,",
    "le terminal repasse en vue split (live). La banniere Interrupted a disparu.",
  ].join("\n"), [
    { testid: "tmux-terminal", label: "Terminal live re-attache (vue split)" },
    { testid: "node-controls", label: "Controles de la node presents (node relancee, live)" },
  ]);

  // ---- Step 3: archived run shows none of the affordances -------------------
  await page.request.post(`${baseURL}/runs/${archiveRun}/commands`, {
    data: { kind: "cleanup_run" },
    headers: { "Content-Type": "application/json" },
  });
  // Reload so the canvas starts empty: otherwise the previous (live) run's
  // `worker` node lingers on the canvas and `getByText("worker")` can select it
  // before the canvas switches to the archived run — a stale-selection race.
  await page.reload();
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 15_000 });
  // Archived runs live in a collapsed "Archived" section — expand it if the run
  // row isn't already on screen, then select the archived run's worker node.
  const archivedToggle = page.getByTestId("run-archived-toggle");
  await expect(archivedToggle).toBeVisible({ timeout: 8_000 });
  const archivedRow = page.getByText(archiveRun.slice(0, 20)).first();
  if (!(await archivedRow.isVisible().catch(() => false))) {
    await archivedToggle.click();
    await expect(archivedRow).toBeVisible({ timeout: 5_000 });
  }
  await openWorker(page, archiveRun);
  await expect(page.getByTestId("terminal-minimized")).toBeVisible({ timeout: 8_000 });
  await expect(page.getByTestId("node-controls")).toHaveCount(0);
  await expect(page.getByTestId("interrupted-reopen-btn")).toHaveCount(0);
  await expect(page.getByTestId("play-retry-btn")).toHaveCount(0);
  await expect(page.getByTestId("mark-complete-btn")).toHaveCount(0);

  await annotateAndShoot(page, "3-archived-no-affordances.png", [
    "Etape 3 — Variante archivee : sur un run archive (worktree + session detruits),",
    "aucune affordance de reprise n'apparait (ni controles, ni Reopen, ni Mark complete). Terminal minimise.",
  ].join("\n"), [
    { testid: "terminal-minimized", label: "Terminal minimise ; aucun bouton de reprise" },
  ]);
});
