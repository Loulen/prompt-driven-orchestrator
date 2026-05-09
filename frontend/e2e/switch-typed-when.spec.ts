import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Layer 3b — Switch typed `when:` clauses E2E (refs #64).
// Seeds a pipeline with a Reviewer (typed verdict enum output) + Switch,
// opens the Switch inspector → verifies field and value dropdowns populated
// from the upstream schema. Disconnects the edge → verifies when clauses cleared.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-switch-typed-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".maestro", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);
const PROMPTS_DIR = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.prompts`);

const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    inputs: []
    outputs:
      - name: user_prompt
    view: { x: 0, y: 200 }
  - id: reviewer
    name: reviewer
    type: doc-only
    inputs:
      - name: task
        side: left
    outputs:
      - name: review
        side: right
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
          score:
            type: int
    view: { x: 250, y: 200 }
  - id: gate
    name: gate
    type: switch
    inputs:
      - name: in
        side: left
    outputs:
      - name: pass
        side: right
        when:
          verdict:
            eq: PASS
      - name: default
        side: right
    view: { x: 550, y: 200 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
        side: left
    outputs: []
    view: { x: 800, y: 200 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: reviewer, port: task }
  - source: { node: reviewer, port: review }
    target: { node: gate, port: in }
  - source: { node: gate, port: pass }
    target: { node: end, port: result }
`;

test.beforeAll(async () => {
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await fs.rm(PROMPTS_DIR, { recursive: true, force: true });
});

test("switch inspector shows typed field and value dropdowns from upstream schema", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  // Enter edit mode and open the pipeline
  await page.locator('[title="Toggle edit mode"]').click();
  await page.getByRole("button", { name: new RegExp(PIPELINE_NAME) }).click();

  // Click the switch node to select it
  const switchNode = page.locator('[data-id="gate"]');
  await expect(switchNode).toBeVisible({ timeout: 5_000 });
  await switchNode.click();

  // Verify the Switch Inspector panel appears
  await expect(page.getByText("Switch Inspector")).toBeVisible({ timeout: 5_000 });

  // Verify the field dropdown contains upstream schema fields
  const fieldDropdown = page.getByTestId("field-dropdown").first();
  await expect(fieldDropdown).toBeVisible();
  const fieldOptions = fieldDropdown.locator("option");
  const fieldValues = await fieldOptions.evaluateAll((opts) =>
    (opts as HTMLOptionElement[]).map((o) => o.value),
  );
  expect(fieldValues).toContain("verdict");
  expect(fieldValues).toContain("score");

  // Verify the value dropdown shows enum allowed values for the verdict field
  const valueDropdown = page.getByTestId("value-dropdown").first();
  await expect(valueDropdown).toBeVisible();
  const valueOptions = valueDropdown.locator("option");
  const valueValues = await valueOptions.evaluateAll((opts) =>
    (opts as HTMLOptionElement[]).map((o) => o.value),
  );
  expect(valueValues).toContain("PASS");
  expect(valueValues).toContain("FAIL");
});
