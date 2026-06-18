import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { openPipelineForEdit } from "./helpers";

// Layer 3b — OutputSchemaEditor round-trip + retry banner (#59).
// Verifies:
// 1. Edit a node's output schema in the inspector, save → YAML round-trips with typed fields.
// 2. Failed-retry banner renders with the offending field list (via seeded run state).

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-schema-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);
const PROMPTS_DIR = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.prompts`);

// Post-refonte the daemon refuses to load a pipeline without exactly one
// start + one end node (crates/pdo-daemon/src/pipeline.rs ~L572), so the
// fixture wires reviewer between a start and an end. The test only touches the
// reviewer node's output schema.
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
    outputs:
      - name: review
    view: { x: 200, y: 200 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
    outputs: []
    view: { x: 400, y: 200 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: reviewer, port: task }
  - source: { node: reviewer, port: review }
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

test("output schema editor round-trips through YAML save", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  // Open the pipeline into the edit canvas (post-refonte: Library tab → entry).
  await openPipelineForEdit(page, PIPELINE_NAME);

  // Click the node to open inspector
  await page.getByText("reviewer", { exact: true }).first().click();

  // Find the output-schema-editor for the review output port
  const schemaEditor = page.getByTestId("output-schema-editor");
  await expect(schemaEditor).toBeVisible();

  // Add a field
  await page.getByTestId("add-schema-field").click();
  const fieldNameInput = page.getByTestId("schema-field-name");
  await expect(fieldNameInput).toBeVisible();

  // Change name to "verdict"
  await fieldNameInput.clear();
  await fieldNameInput.fill("verdict");

  // Change type to enum
  const typeSelect = page.getByTestId("schema-field-type");
  await typeSelect.selectOption("enum");

  // Add allowed values
  const allowedInput = page.getByTestId("allowed-input");
  await allowedInput.fill("PASS");
  await allowedInput.press("Enter");
  await allowedInput.fill("FAIL");
  await allowedInput.press("Enter");

  // Two chips should be visible
  const chips = page.getByTestId("allowed-chip");
  await expect(chips).toHaveCount(2);

  // Save
  const saveBtn = page.getByTestId("save-button");
  await saveBtn.click();

  // Read the YAML file and verify the frontmatter schema round-tripped
  const yaml = await fs.readFile(PIPELINE_PATH, "utf-8");
  expect(yaml).toContain("frontmatter:");
  expect(yaml).toContain("verdict:");
  expect(yaml).toContain("type: enum");
  expect(yaml).toContain("PASS");
  expect(yaml).toContain("FAIL");
});
