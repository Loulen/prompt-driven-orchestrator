import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Layer 3b — Library node round-trip preserves inputs/outputs (#71).
// Verifies: configure a node with 2 inputs + 1 typed output, save to library,
// reload UI, drag back from library → ports + frontmatter schemas preserved.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-lib-ports-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".maestro", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);
const PROMPTS_DIR = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.prompts`);

const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: reviewer
    name: reviewer
    type: doc-only
    inputs:
      - name: code
      - name: context
        repeated: true
    outputs:
      - name: review
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
          score:
            type: int
    view: { x: 200, y: 200 }
edges: []
`;

test.beforeAll(async () => {
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await fs.rm(PROMPTS_DIR, { recursive: true, force: true });
  // Clean up library entry
  const libDir = path.join(
    process.env.HOME ?? "",
    ".maestro",
    "library",
  );
  const libFile = path.join(libDir, "reviewer.yaml");
  await fs.rm(libFile, { force: true });
});

test("save node to library preserves ports and frontmatter schema", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  // Enter edit mode and open the pipeline
  await page.locator('[title="Toggle edit mode"]').click();
  await page.getByRole("button", { name: new RegExp(PIPELINE_NAME) }).click();

  // Click the reviewer node to select it
  await page.getByText("reviewer", { exact: true }).first().click();

  // The star button should show "Save to library" (outline state)
  const starBtn = page.locator('[title="Save to library"]');
  await expect(starBtn).toBeVisible({ timeout: 3_000 });

  // Click the star to save to library
  await starBtn.click();

  // After saving, star tooltip should change to "In your library — synced"
  await expect(
    page.locator('[title="In your library — synced"]'),
  ).toBeVisible({ timeout: 5_000 });

  // Verify via API that the library entry has full port data
  const resp = await fetch(`${baseURL}/library`);
  expect(resp.status).toBe(200);
  const entries = (await resp.json()) as Array<Record<string, unknown>>;
  const entry = entries.find((e) => e.name === "reviewer");
  expect(entry).toBeTruthy();

  // Assert inputs preserved
  const inputs = entry!.inputs as Array<Record<string, unknown>>;
  expect(inputs).toHaveLength(2);
  expect(inputs[0].name).toBe("code");
  expect(inputs[1].name).toBe("context");
  expect(inputs[1].repeated).toBe(true);

  // Assert output frontmatter preserved
  const outputs = entry!.outputs as Array<Record<string, unknown>>;
  expect(outputs).toHaveLength(1);
  expect(outputs[0].name).toBe("review");
  const fm = outputs[0].frontmatter as Record<
    string,
    Record<string, unknown>
  >;
  expect(fm).toBeTruthy();
  expect(fm.verdict.type).toBe("enum");
  expect(fm.verdict.allowed).toEqual(["PASS", "FAIL"]);
  expect(fm.score.type).toBe("int");
});

test("instantiating from library restores ports and frontmatter", async ({
  baseURL,
}) => {
  // First ensure the library entry exists (from previous test or create via API)
  const checkResp = await fetch(`${baseURL}/library`);
  const entries = (await checkResp.json()) as Array<Record<string, unknown>>;
  if (!entries.find((e) => e.name === "reviewer")) {
    await fetch(`${baseURL}/library`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        name: "reviewer",
        type: "doc-only",
        inputs: [
          { name: "code", repeated: false },
          { name: "context", repeated: true },
        ],
        outputs: [
          {
            name: "review",
            repeated: false,
            frontmatter: {
              verdict: { type: "enum", allowed: ["PASS", "FAIL"] },
              score: { type: "int" },
            },
          },
        ],
        interactive: false,
        prompt: "You review code.",
      }),
    });
  }

  // Instantiate via API and verify full round-trip
  const resp = await fetch(
    `${baseURL}/library/${encodeURIComponent("reviewer")}/instantiate`,
    { method: "POST" },
  );
  expect(resp.status).toBe(200);
  const result = (await resp.json()) as Record<string, unknown>;
  const spec = result.spec as Record<string, unknown>;

  const outputs = spec.outputs as Array<Record<string, unknown>>;
  expect(outputs).toHaveLength(1);
  const fm = outputs[0].frontmatter as Record<
    string,
    Record<string, unknown>
  >;
  expect(fm).toBeTruthy();
  expect(fm.verdict.type).toBe("enum");
  expect(fm.verdict.allowed).toEqual(["PASS", "FAIL"]);
  expect(fm.score.type).toBe("int");

  const inputs = spec.inputs as Array<Record<string, unknown>>;
  expect(inputs).toHaveLength(2);
  expect(inputs[1].repeated).toBe(true);
});
