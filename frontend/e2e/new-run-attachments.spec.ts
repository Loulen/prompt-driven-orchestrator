import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as os from "node:os";
import * as path from "node:path";
import { E2E_TARGET_REPO, pickPipeline } from "./helpers";

// Layer 3b — « New run » attachments (#779).
// Verifies: the drop zone accepts a non-image file next to an image, shows it as
// a chip with the running counter, sends images in `images` and the rest in
// `files`, and the created Run carries both in `start_node`.

const PIPELINE_NAME = `e2e-attachments-${process.pid}-${Date.now()}`;
// Instance pipelines live under `$HOME/.pdo/pipelines` (ADR-0059), not in the
// repo: a file seeded under `<repo>/.pdo/pipelines` never reaches the list.
const PIPELINE_DIR = path.join(os.homedir(), ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);

// Start → End only: nothing spawns, the run completes on its own.
const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - { name: user_prompt, side: bottom }
    view: { x: 100, y: 0 }
  - id: end
    name: End
    type: end
    inputs:
      - { name: result, side: top }
    view: { x: 100, y: 200 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: end, port: result }
`;

test.beforeAll(async () => {
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
});

test("a dropped file becomes a chip and reaches the run as `files`", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  await page.getByRole("button", { name: "New Run" }).click();
  await expect(page.getByTestId("target-repo-input")).toBeVisible();
  await page.getByTestId("target-repo-input").fill(E2E_TARGET_REPO);
  await pickPipeline(page, PIPELINE_NAME);
  await page.getByPlaceholder(/free-text prompt/i).fill("e2e attachments");

  // Empty state: the helper names the budget the daemon enforces.
  const zone = page.getByTestId("image-drop-zone");
  await expect(zone).toBeVisible();
  await expect(page.getByText(/up to .* MB per run/i)).toBeVisible();

  // The hidden <input type=file> has no accept filter any more — any file goes.
  const input = page.getByTestId("image-file-input");
  await expect(input).not.toHaveAttribute("accept");
  await input.setInputFiles([
    {
      name: "SPEC-779.md",
      mimeType: "text/markdown",
      buffer: Buffer.from("# Spec 779\n\nImplement the prototype.\n"),
    },
    {
      name: "proto.png",
      mimeType: "image/png",
      // 1×1 transparent PNG.
      buffer: Buffer.from(
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=",
        "base64",
      ),
    },
  ]);

  const chip = page.getByTestId("file-chip");
  await expect(chip).toHaveCount(1);
  await expect(chip).toContainText("MD");
  await expect(chip).toContainText("SPEC-779.md");
  await expect(page.getByTestId("image-thumbnail")).toHaveCount(1);
  await expect(page.getByTestId("attachments-counter")).toContainText("1 image, 1 file");
  await expect(page.getByTestId("attachments-meter")).toBeVisible();

  // Launch: one multipart request, `images` and `files` split by MIME.
  const [request] = await Promise.all([
    page.waitForRequest((r) => r.url().endsWith("/runs") && r.method() === "POST"),
    page.getByTestId("launch-button").click(),
  ]);
  // Chromium does not expose the body of a multipart request carrying files
  // (`postData()` is empty), so the field split is asserted daemon-side below.
  expect(request.headers()["content-type"]).toContain("multipart/form-data");

  // The daemon wrote both into `_input/` and lists them on the Start node: the PNG
  // under `input_images` (field `images`), the Markdown under `input_files` (`files`).
  await expect
    .poll(
      async () => {
        const resp = await page.request.get(`${baseURL}/runs`);
        const runs = (await resp.json()) as Array<{ run_id: string; pipeline_name: string }>;
        const mine = runs.find((r) => r.pipeline_name === PIPELINE_NAME);
        if (!mine) return null;
        const detail = await (await page.request.get(`${baseURL}/runs/${mine.run_id}`)).json();
        return {
          images: detail.start_node?.input_images,
          files: detail.start_node?.input_files,
        };
      },
      { timeout: 10_000 },
    )
    .toEqual({ images: ["proto.png"], files: ["SPEC-779.md"] });
});
