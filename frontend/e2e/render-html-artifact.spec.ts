import { test, expect, type Page } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Layer 3b — html output port rendering in MarkdownArtifactModal (#333,
// ADR-0028). This is the PERMANENT home of the "the script never runs"
// invariant (resilience is not a Happy Path — see docs/test-scenarios/README).
//
// It proves, against a real daemon + browser, that an `html` output port whose
// artifact contains a hostile `<script>` + `<img onerror>` payload is rendered
// in an `<iframe sandbox="" srcDoc=...>` where:
//   - the sandbox attribute is a literal empty allow-list (no `allow-scripts`),
//   - the benign HTML/CSS still renders,
//   - no script executes (no dialog, no `window.__pdo_pwned`, no page error).

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-render-html-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);
const PROMPTS_DIR = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.prompts`);

// One `start` (auto-emits the prompt) → `designer` (an html output port) →
// `end`. `designer` spawns under the tmux stub (kept "running" by `sleep`); we
// seed its `output.html` on disk and read it back through the node-IO API.
const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - { name: user_prompt, side: right }
    view: { x: 0, y: 200 }
  - id: designer
    name: designer
    type: doc-only
    prompt_file: ${PIPELINE_NAME}.prompts/designer.md
    inputs:
      - { name: in, side: left }
    outputs:
      - name: report
        port_type: html
    view: { x: 200, y: 200 }
  - id: end
    name: End
    type: end
    inputs:
      - { name: result, side: top }
    view: { x: 400, y: 200 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: designer, port: in }
  - source: { node: designer, port: report }
    target: { node: end, port: result }
`;

const ROLE_PROMPT = "You write HTML reports.\n";

// A hostile artifact: it tries to run script two ways (a bare <script> and an
// <img onerror>) and to escape to the parent window. Under `sandbox=""` neither
// path can execute. The benign <h1> must still render.
const HOSTILE_HTML = `<!doctype html>
<html>
  <head><style>h1 { color: rebeccapurple; }</style></head>
  <body>
    <h1 data-testid="report-heading">Quarterly Report</h1>
    <script>window.top.__pdo_pwned = 1; window.__pdo_pwned = 1;</script>
    <img src="x" onerror="window.top.__pdo_pwned = 1" />
  </body>
</html>
`;

let runId: string;

test.beforeAll(async () => {
  await fs.mkdir(PROMPTS_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
  await fs.writeFile(path.join(PROMPTS_DIR, "designer.md"), ROLE_PROMPT);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await fs.rm(PROMPTS_DIR, { recursive: true, force: true });
  if (runId) {
    const { execSync } = await import("node:child_process");
    try {
      // kill-session only — never `kill <pid>` (would take down the tmux server).
      execSync(`tmux kill-session -t pdo-${runId}-designer-iter-1`, {
        stdio: "ignore",
      });
    } catch {
      // session may already be dead
    }
  }
});

async function createRunAndSeedArtifact(baseURL: string) {
  const resp = await fetch(`${baseURL}/runs`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ pipeline: PIPELINE_NAME, input: "html layer3b" }),
  });
  expect(resp.status).toBe(201);
  const json = await resp.json();
  runId = json.run_id;

  // The daemon resolves an html output port to
  // `<node>/iter-1/<port>/output.html` (crates/pdo-daemon/src/blackboard.rs).
  const portDir = path.join(
    WORKSPACE_ROOT,
    ".pdo",
    "runs",
    runId,
    "worktree",
    ".pdo",
    "artifacts",
    "designer",
    "iter-1",
    "report",
  );
  await fs.mkdir(portDir, { recursive: true });
  await fs.writeFile(path.join(portDir, "output.html"), HOSTILE_HTML);
  return runId;
}

async function openNode(page: Page) {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });
  await page
    .getByText(runId.slice(0, 20))
    .first()
    .click({ timeout: 5_000, position: { x: 5, y: 5 } });

  const reactFlow = page.locator(".react-flow");
  await expect(reactFlow).toBeVisible({ timeout: 5_000 });
  await page.waitForTimeout(500);

  const node = page.getByText("designer", { exact: true }).first();
  await expect(node).toBeVisible({ timeout: 3_000 });
  await node.click();

  // Opening a live run auto-selects the running node with its terminal expanded
  // fullscreen, hiding the Inputs/Outputs pane. Collapse it to reveal the port
  // cards.
  const detailsPane = page.locator('[data-testid="details-pane"]');
  const expandToggle = page.locator('[data-testid="term-expand"]');
  await expect(expandToggle).toBeVisible({ timeout: 5_000 });
  if (!(await detailsPane.isVisible())) {
    await expandToggle.click();
  }
  await expect(detailsPane).toBeVisible({ timeout: 5_000 });
}

test("renders an html artifact in a scriptless sandboxed iframe", async ({
  page,
  baseURL,
}) => {
  const pageErrors: string[] = [];
  page.on("pageerror", (e) => pageErrors.push(String(e)));
  // A hostile payload must never trigger a dialog.
  page.on("dialog", async (d) => {
    pageErrors.push(`unexpected dialog: ${d.message()}`);
    await d.dismiss();
  });

  await createRunAndSeedArtifact(baseURL!);
  await openNode(page);

  // Open the html port row.
  const portRow = page
    .locator("button.port-row")
    .filter({ hasText: /^report/ });
  await expect(portRow).toBeVisible({ timeout: 5_000 });
  await portRow.click();

  // The artifact renders inside the sandboxed iframe.
  const frame = page.locator('[data-testid="html-artifact-frame"]');
  await expect(frame).toBeVisible({ timeout: 5_000 });

  // Security invariant: the sandbox allow-list is literally empty — scripts are
  // never enabled.
  const sandbox = await frame.getAttribute("sandbox");
  expect(sandbox).toBe("");
  expect(sandbox ?? "").not.toContain("allow-scripts");

  // The HTML flows through `srcDoc`, so the raw hostile text is present as the
  // attribute value (but inert).
  const srcdoc = await frame.getAttribute("srcdoc");
  expect(srcdoc).toContain("Quarterly Report");
  expect(srcdoc).toContain("<script>");

  // The benign HTML/CSS actually renders inside the (null-origin) frame.
  const heading = page
    .frameLocator('[data-testid="html-artifact-frame"]')
    .locator('[data-testid="report-heading"]');
  await expect(heading).toHaveText("Quarterly Report", { timeout: 5_000 });

  // Give any (blocked) script/onerror a chance to fire, then prove none did.
  await page.waitForTimeout(300);
  expect(
    await page.evaluate(
      () => (window as Window & { __pdo_pwned?: number }).__pdo_pwned,
    ),
  ).toBeUndefined();
  expect(pageErrors).toEqual([]);
});
