import { test, expect } from "@playwright/test";
import type { Route } from "@playwright/test";

// A Review link (`/runs/<id>/review?file=<path>`) lands on the file it names,
// even when that file sits far below the fold of a long diff: the page scrolls
// to its card once the diff is shown, and stays there while the cards above it
// render their bodies and grow.
//
// The daemon serves the page; the Run, its refs, its diff and its files are
// answered by the browser (`page.route`), so the spec needs no real Run.

const RUN_ID = "20260101-000000-landing";
const COUNT = 14;
const TARGET = "proto/index.html";

const lines = (n: number, prefix: string) =>
  Array.from({ length: n }, (_, i) => ({ kind: "add", content: `${prefix} line ${i + 1}`, old_no: null, new_no: i + 1 }));

function file(path: string) {
  const ls = lines(40, path);
  return {
    old_path: null,
    new_path: path,
    status: "added",
    binary: false,
    additions: ls.length,
    deletions: 0,
    hunks: [{ old_start: 0, old_lines: 0, new_start: 1, new_lines: ls.length, header: "", lines: ls }],
  };
}

const paths = [
  ...Array.from({ length: COUNT - 1 }, (_, i) => `src/file-${String(i + 1).padStart(2, "0")}.ts`),
  TARGET,
];
const files = paths.map(file);

const run = {
  run_id: RUN_ID,
  status: "completed",
  pipeline_name: "landing",
  input: "x",
  started_at: "2026-01-01T00:00:00Z",
  completed_at: "2026-01-01T01:00:00Z",
  nodes: {},
  edges: [],
  node_defs: [],
  loop_states: {},
  foreach_states: {},
  collection_states: {},
  switch_states: {},
  sandbox: "off",
  sessions_spawned: 0,
  review_comments: [],
};

const refs = {
  default_from: "fork",
  default_to: "tip",
  refs: [
    { id: "fork", kind: "fork", label: "Fork point", git_ref: "a", sha: "a".repeat(40) },
    { id: "tip", kind: "tip", label: "Run tip", git_ref: `pdo/run-${RUN_ID}`, sha: "b".repeat(40) },
  ],
  deliveries: [],
};

const diff = {
  from_ref: "fork",
  to_ref: "tip",
  from_sha: "a".repeat(40),
  to_sha: "b".repeat(40),
  three_dot: true,
  files,
  additions: files.reduce((s, f) => s + f.additions, 0),
  deletions: 0,
  files_changed: files.length,
};

test("a Review link lands on its file below the fold", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 860 });
  const base = `/runs/${RUN_ID}`;
  await page.route(
    (url) => url.pathname.startsWith(base) && !url.pathname.endsWith("/review"),
    async (route: Route) => {
      const url = new URL(route.request().url());
      const rest = url.pathname.slice(base.length);
      if (rest === "") return route.fulfill({ json: run });
      if (rest === "/refs") return route.fulfill({ json: refs });
      if (rest === "/diff/structured") {
        // A slow diff, like a real one on a busy daemon.
        await new Promise((r) => setTimeout(r, 300));
        return route.fulfill({ json: diff });
      }
      if (rest === "/file") {
        // A card's full content arrives a little later (cards near the viewport).
        await new Promise((r) => setTimeout(r, 150));
        const p = url.searchParams.get("path") ?? "";
        return route.fulfill({ body: lines(40, p).map((l) => l.content).join("\n") + "\n", contentType: "text/plain" });
      }
      if (rest.startsWith("/review/comments")) return route.fulfill({ json: { comments: [] } });
      return route.fulfill({ json: [] });
    },
  );

  await page.goto(`${base}/review?file=${encodeURIComponent(TARGET)}`);
  const card = page.locator('[data-testid="review-main"] [data-path="' + TARGET + '"]').first();
  await expect(page.getByTestId("review-main")).toBeVisible();
  await expect(card).toBeAttached();

  // The card's top settles inside the viewport of the diff, near its top.
  await expect
    .poll(
      async () => {
        const box = await card.boundingBox();
        return box ? Math.round(box.y) : null;
      },
      { timeout: 8_000 },
    )
    .toBeLessThan(300);
  // And stays there once the cards above have grown.
  await page.waitForTimeout(1500);
  const box = await card.boundingBox();
  expect(box?.y ?? Infinity).toBeLessThan(300);
  expect(box?.y ?? -Infinity).toBeGreaterThanOrEqual(0);

  // The reader scrolls away: the page lets them go.
  await page.getByTestId("review-main").hover();
  await page.mouse.wheel(0, -1200);
  await expect.poll(async () => (await card.boundingBox())?.y ?? 0).toBeGreaterThan(600);
  await page.waitForTimeout(500);
  expect((await card.boundingBox())?.y ?? 0).toBeGreaterThan(600);
});
