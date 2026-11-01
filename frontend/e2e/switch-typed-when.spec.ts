import { test, expect } from "@playwright/test";
import { openPipelineForEdit } from "./helpers";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import * as os from "node:os";

// Layer 3b — Typed `when:` clauses E2E (refs #64).
//
// Post-canvas-refonte (ADR-0011) there is no "Switch Inspector" panel: a switch
// is just an output port with one conditional edge per branch, and the `when:`
// predicate is authored per-edge in the EdgeDetailPanel. So this seeds a typed
// `review` output (verdict enum + score int) with a conditional edge to End,
// opens the pipeline, selects that edge, and verifies the when-editor's field
// dropdown is populated from the upstream output schema and the value dropdown
// from the selected enum field's allowed values.

const PIPELINE_NAME = `e2e-switch-typed-${process.pid}-${Date.now()}`;
// Instance pipelines live under `$HOME/.pdo/pipelines` (ADR-0059), not in the
// repo: a file seeded under `<repo>/.pdo/pipelines` never reaches the list.
const PIPELINE_DIR = path.join(os.homedir(), ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);

// The `when:` lives on the edge (edges[1]) → its EditCanvas id is `e-1`, its
// condition chip `edge-condition-label-e-1`.
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
    type: agent
    isolated_worktree: false
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
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  // Open the pipeline into the edit canvas via the Library tab (post-refonte:
  // no edit toggle).
  await openPipelineForEdit(page, PIPELINE_NAME);

  // Wait for the canvas to render the nodes/edges.
  await expect(page.getByText("reviewer", { exact: true }).first()).toBeVisible({
    timeout: 5_000,
  });

  // Select the conditional reviewer→end edge (edges[1]) through its condition
  // chip: a click on the chip selects its edge (#845). The Start and End
  // markers are narrower than a card, so the route bends and the centre of
  // its hit path's box is not on the stroke.
  await page.getByTestId("edge-condition-label-e-1").click({ timeout: 5_000 });

  // The edge detail panel opens with the when-editor (the edge already carries
  // a `verdict: PASS` predicate, so one condition row is present).
  await expect(page.getByTestId("edge-detail-panel")).toBeVisible({ timeout: 5_000 });

  // The section is « Conditions », and « Add condition » shows without hover.
  const panel = page.getByTestId("edge-detail-panel");
  await expect(panel.getByText("Conditions", { exact: true })).toBeVisible();
  await expect(panel.getByRole("button", { name: "Add condition" })).toBeVisible();

  // Field dropdown lists the upstream output schema fields (+ the region `iter`),
  // grouped by origin, each with its type and allowed values.
  const fieldDropdown = page.getByTestId("field-dropdown").first();
  await expect(fieldDropdown).toBeVisible();
  await fieldDropdown.getByRole("button").click();
  const fieldList = page.getByRole("listbox", { name: "Field" });
  await expect(fieldList.getByRole("option", { name: /^verdict/ })).toContainText("enum · PASS | FAIL");
  await expect(fieldList.getByRole("option", { name: /^score/ })).toContainText("int");
  await expect(fieldList).toContainText("Output · review");
  await page.keyboard.press("Escape");

  // The verdict field is an enum, so the value dropdown shows its allowed values.
  const valueDropdown = page.getByTestId("value-dropdown").first();
  await expect(valueDropdown).toBeVisible();
  await valueDropdown.getByRole("button").click();
  const valueList = page.getByRole("listbox", { name: "Value" });
  await expect(valueList.getByRole("option", { name: "PASS" })).toBeVisible();
  await expect(valueList.getByRole("option", { name: "FAIL" })).toBeVisible();
});
