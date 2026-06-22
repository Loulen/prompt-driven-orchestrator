import { test, expect, type Page } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { expectNonZeroBBox } from "./assertions";

// Layer 3b — mermaid rendering in MarkdownArtifactModal (#240, ADR-0013).
// jsdom can't execute mermaid (no SVG getBBox), so this is the meaningful
// automated layer for the real render path. It proves: a valid ```mermaid fence
// renders an inline dark-themed <svg>; invalid mermaid degrades to raw
// <pre><code> (never blank, never a thrown error); a hostile payload does NOT
// execute script under securityLevel:'strict'; and a non-mermaid fence is left
// as a normal code block.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-render-mermaid-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);
const PROMPTS_DIR = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.prompts`);

// A valid pipeline needs exactly one `start` node (one `user_prompt` output) and
// one `end` node (one `result` input); see crates/pdo-daemon/src/pipeline.rs. The
// `start` auto-emits the prompt so `diagrammer` spawns (stubbed by sleep); we seed
// its output artifacts on disk and read them back through the node-IO API.
const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - { name: user_prompt, side: right }
    view: { x: 0, y: 200 }
  - id: diagrammer
    name: diagrammer
    type: doc-only
    prompt_file: ${PIPELINE_NAME}.prompts/diagrammer.md
    inputs:
      - { name: in, side: left }
    outputs:
      - name: good
      - name: complex
      - name: bad
      - name: xss
      - name: plain
    view: { x: 200, y: 200 }
  - id: end
    name: End
    type: end
    inputs:
      - { name: result, side: top }
    view: { x: 400, y: 200 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: diagrammer, port: in }
  - source: { node: diagrammer, port: good }
    target: { node: end, port: result }
`;

const ROLE_PROMPT = "You draw diagrams.\n";

// Per-port artifact bodies. The daemon resolves a markdown output port to
// `<node>/iter-1/<port>/output.md` (see crates/pdo-daemon/src/blackboard.rs);
// the flat `<port>.md` layout in older specs does NOT match that resolver.
const ARTIFACTS: Record<string, string> = {
  good: `## Flow

\`\`\`mermaid
graph TD;
  A[Start] --> B{Decision};
  B -->|yes| C[Ship];
  B -->|no| D[Iterate];
\`\`\`
`,
  complex: `## Sequence

\`\`\`mermaid
sequenceDiagram
  participant U as User
  participant D as Daemon
  U->>D: POST /runs
  D-->>U: 201 run_id
\`\`\`
`,
  bad: `## Broken

\`\`\`mermaid
this is not ::: valid mermaid @@@ ->> nonsense
\`\`\`
`,
  xss: `## Hostile

\`\`\`mermaid
graph LR
  A["<img src=x onerror='window.__mermaidXss=1'>"] --> B[B]
\`\`\`
`,
  plain: `## Plain

\`\`\`ts
const x: number = 1;
\`\`\`
`,
};

let runId: string;

test.beforeAll(async () => {
  process.env.PDO_TMUX_CMD_OVERRIDE = "exec sleep 600";
  await fs.mkdir(PROMPTS_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
  await fs.writeFile(path.join(PROMPTS_DIR, "diagrammer.md"), ROLE_PROMPT);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await fs.rm(PROMPTS_DIR, { recursive: true, force: true });
  delete process.env.PDO_TMUX_CMD_OVERRIDE;
  if (runId) {
    const { execSync } = await import("node:child_process");
    try {
      // kill-session only — never `kill <pid>` (would take down the tmux server).
      execSync(`tmux kill-session -t pdo-${runId}-diagrammer-iter-1`, {
        stdio: "ignore",
      });
    } catch {
      // session may already be dead
    }
  }
});

async function createRunAndSeedArtifacts(baseURL: string) {
  const resp = await fetch(`${baseURL}/runs`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ pipeline: PIPELINE_NAME, input: "mermaid layer3b" }),
  });
  expect(resp.status).toBe(201);
  const json = await resp.json();
  runId = json.run_id;

  const iterDir = path.join(
    WORKSPACE_ROOT,
    ".pdo",
    "runs",
    runId,
    "worktree",
    ".pdo",
    "artifacts",
    "diagrammer",
    "iter-1",
  );
  for (const [port, body] of Object.entries(ARTIFACTS)) {
    const portDir = path.join(iterDir, port);
    await fs.mkdir(portDir, { recursive: true });
    await fs.writeFile(path.join(portDir, "output.md"), body);
  }
  return runId;
}

async function openNode(page: Page) {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });
  await page.getByText(runId.slice(0, 8)).first().click({ timeout: 5_000 });

  const reactFlow = page.locator(".react-flow");
  await expect(reactFlow).toBeVisible({ timeout: 5_000 });
  await page.waitForTimeout(500);

  const node = page.getByText("diagrammer", { exact: true }).first();
  await expect(node).toBeVisible({ timeout: 3_000 });
  await node.click();

  // Opening a live run auto-selects the running node with its terminal expanded
  // fullscreen (App.tsx initialTerminalExpanded), which hides the Inputs/Outputs
  // pane. Collapse the terminal to reveal the output port cards.
  const detailsPane = page.locator('[data-testid="details-pane"]');
  const expandToggle = page.locator('[data-testid="term-expand"]');
  await expect(expandToggle).toBeVisible({ timeout: 5_000 });
  if (!(await detailsPane.isVisible())) {
    await expandToggle.click();
  }
  await expect(detailsPane).toBeVisible({ timeout: 5_000 });
}

async function openPort(page: Page, port: string) {
  const card = page
    .locator("button.port-row")
    .filter({ hasText: new RegExp(`^${port}`) });
  await expect(card).toBeVisible({ timeout: 5_000 });
  await card.click();
  await expect(page.locator(".artifact-markdown")).toBeVisible({
    timeout: 3_000,
  });
}

async function closeModal(page: Page) {
  await page.keyboard.press("Escape");
  await expect(page.locator(".artifact-markdown")).not.toBeVisible({
    timeout: 2_000,
  });
}

test("renders, degrades, secures and ignores non-mermaid fences", async ({
  page,
  baseURL,
}) => {
  // Surface any uncaught page exception — a caught parse/render failure must
  // never bubble up as one (ADR-0013).
  const pageErrors: string[] = [];
  page.on("pageerror", (e) => pageErrors.push(String(e)));
  // A hostile payload must never trigger a dialog.
  page.on("dialog", async (d) => {
    pageErrors.push(`unexpected dialog: ${d.message()}`);
    await d.dismiss();
  });

  await createRunAndSeedArtifacts(baseURL!);
  await openNode(page);

  // 1) Valid flowchart → an <svg> with a non-zero box; the fence text is consumed.
  await openPort(page, "good");
  const goodSvg = page.locator('[data-testid="mermaid-diagram"] svg');
  await expect(goodSvg).toBeVisible({ timeout: 10_000 });
  await expectNonZeroBBox(goodSvg);
  await expect(page.locator(".artifact-markdown")).not.toContainText(
    "graph TD",
  );
  await closeModal(page);

  // 2) Sequence diagram → rendered with the dark theme (node fill not white).
  await openPort(page, "complex");
  const complexSvg = page.locator('[data-testid="mermaid-diagram"] svg');
  await expect(complexSvg).toBeVisible({ timeout: 10_000 });
  await expectNonZeroBBox(complexSvg);
  const fills = await complexSvg.evaluate((svg) => {
    const out: string[] = [];
    svg
      .querySelectorAll<SVGElement>("rect, polygon, path, circle")
      .forEach((el) => {
        const f = getComputedStyle(el).fill;
        if (f && f !== "none") out.push(f);
      });
    return out;
  });
  const toRgb = (s: string) =>
    s.match(/\d+/g)?.slice(0, 3).map(Number) ?? null;
  const isLight = (rgb: number[]) =>
    (rgb[0] === 255 && rgb[1] === 255 && rgb[2] === 255) || // white
    (rgb[0] === 236 && rgb[1] === 236 && rgb[2] === 255); // mermaid default #ECECFF
  const rgbs = fills.map(toRgb).filter((v): v is number[] => v !== null);
  expect(rgbs.length).toBeGreaterThan(0);
  expect(rgbs.some((rgb) => !isLight(rgb))).toBe(true); // at least one dark shape
  expect(rgbs.every((rgb) => !isLight(rgb))).toBe(true); // no light-theme leak
  await closeModal(page);

  // 3) Invalid mermaid → graceful degrade to raw <pre><code>, no diagram.
  await openPort(page, "bad");
  const errFallback = page.locator('[data-testid="mermaid-error"]');
  await expect(errFallback).toBeVisible({ timeout: 10_000 });
  await expect(errFallback).toContainText("this is not ::: valid mermaid");
  await expect(
    page.locator('[data-testid="mermaid-diagram"] svg'),
  ).toHaveCount(0);
  await closeModal(page);

  // 4) Security (strict): hostile payload must not execute script.
  expect(await page.evaluate(() => (window as Window & { __mermaidXss?: number }).__mermaidXss)).toBeUndefined();
  await openPort(page, "xss");
  // Wait for the modal to settle into either a diagram or the fallback.
  await expect(
    page.locator(
      '[data-testid="mermaid-diagram"], [data-testid="mermaid-error"]',
    ),
  ).toBeVisible({ timeout: 10_000 });
  await page.waitForTimeout(300); // give any (blocked) onerror a chance to fire
  expect(await page.evaluate(() => (window as Window & { __mermaidXss?: number }).__mermaidXss)).toBeUndefined();
  await closeModal(page);

  // 5) Negative control: a non-mermaid fence stays a normal code block.
  await openPort(page, "plain");
  const md = page.locator(".artifact-markdown");
  await expect(md).toContainText("const x: number = 1;");
  await expect(
    page.locator('[data-testid="mermaid-diagram"]'),
  ).toHaveCount(0);
  await expect(page.locator('[data-testid="mermaid-error"]')).toHaveCount(0);
  await closeModal(page);

  expect(pageErrors).toEqual([]);
});
