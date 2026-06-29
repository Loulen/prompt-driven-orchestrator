import { test, expect } from "@playwright/test";
import { openPipelineForEdit } from "./helpers";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Layer 3b — Typed `when:` clauses E2E (refs #64).
//
// Post-canvas-refonte (ADR-0011) there is no "Switch Inspector" panel: a switch
// is just an output port with one conditional edge per branch, and the `when:`
// predicate is authored per-edge in the EdgeDetailPanel. So this seeds a typed
// `review` output (verdict enum + score int) with a conditional edge to End,
// opens the pipeline, selects that edge, and verifies the when-editor's field
// dropdown is populated from the upstream output schema and the value dropdown
// from the selected enum field's allowed values.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-switch-typed-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);

// Nodes share one x so the conditional reviewer→end edge is a straight vertical
// segment — its (transparent, wide) hit path is then reliably centred for the
// click that selects it. The `when:` lives on the edge (edges[1]) → its
// EditCanvas id is `e-1`, exposed as `orthogonal-edge-hit-e-1`.
const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - { name: user_prompt, side: bottom }
    view: { x: 200, y: 0 }
  - id: reviewer
    name: reviewer
    type: doc-only
    inputs:
      - { name: task, side: top }
    outputs:
      - name: review
        side: bottom
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
          score:
            type: int
    view: { x: 200, y: 160 }
  - id: end
    name: End
    type: end
    inputs:
      - { name: result, side: top }
    view: { x: 200, y: 320 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: reviewer, port: task }
  - source: { node: reviewer, port: review }
    target: { node: end, port: result }
    when:
      verdict: { eq: PASS }
`;

test.beforeAll(async () => {
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
});

test("when-editor shows typed field and value dropdowns from upstream schema", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  // Open the pipeline into the edit canvas via the Library tab (post-refonte:
  // no edit toggle).
  await openPipelineForEdit(page, PIPELINE_NAME);

  // Wait for the canvas to render the nodes/edges.
  await expect(page.getByText("reviewer", { exact: true }).first()).toBeVisible({
    timeout: 5_000,
  });

  // Select the conditional reviewer→end edge (edges[1]). Its hit path is a
  // transparent SVG stroke (no fill), which Playwright treats as not "visible",
  // so force the click — react-flow's onEdgeClick fires on the bubbled event.
  await page.getByTestId("orthogonal-edge-hit-e-1").click({ force: true, timeout: 5_000 });

  // The edge detail panel opens with the when-editor (the edge already carries
  // a `verdict: PASS` predicate, so one condition row is present).
  await expect(page.getByTestId("edge-detail-panel")).toBeVisible({ timeout: 5_000 });

  // Field dropdown lists the upstream output schema fields (+ the region `iter`).
  const fieldDropdown = page.getByTestId("field-dropdown").first();
  await expect(fieldDropdown).toBeVisible();
  const fieldValues = await fieldDropdown
    .locator("option")
    .evaluateAll((opts) => (opts as HTMLOptionElement[]).map((o) => o.value));
  expect(fieldValues).toContain("verdict");
  expect(fieldValues).toContain("score");

  // The verdict field is an enum, so the value dropdown shows its allowed values.
  const valueDropdown = page.getByTestId("value-dropdown").first();
  await expect(valueDropdown).toBeVisible();
  const valueValues = await valueDropdown
    .locator("option")
    .evaluateAll((opts) => (opts as HTMLOptionElement[]).map((o) => o.value));
  expect(valueValues).toContain("PASS");
  expect(valueValues).toContain("FAIL");
});
