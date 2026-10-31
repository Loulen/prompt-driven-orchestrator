import { test, expect, type Page } from "@playwright/test";

// Stats › Cost in a real browser at the narrowest window the interface holds
// (1024 px) and its default scale (120 %): a jsdom test cannot measure a column.
// The e2e daemon has no priced Run, so `/stats/cost` is answered here with four
// harnesses and a long pipeline name: the case that squeezed the Name column.

const PIPELINE_NAME = "implement-review-with-a-pipeline-name-long-enough-to-be-cut";

const EMPTY = {
  usd: null,
  average_usd: null,
  median_usd: null,
  estimated: true,
  partial: false,
  executions: 0,
  readable: 0,
  unknown: 0,
  unpriced_models: [],
  missing_reasons: [],
  harnesses: [],
};

const HARNESSES = ["claude", "codex", "copilot", "opencode"];

function harness(name: string, usd: number) {
  return { ...EMPTY, harness: name, usd, executions: 3, readable: 3, average_usd: usd / 3, median_usd: usd / 3 };
}

function entity(id: string, name: string, usd: number) {
  return {
    ...EMPTY,
    id,
    name,
    usd,
    executions: 12,
    readable: 12,
    average_usd: usd / 12,
    median_usd: 1234.5678,
    harnesses: HARNESSES.map((h, i) => harness(h, usd / (i + 2))),
    by_period: [],
    nodes: [],
  };
}

const pipeline = {
  ...entity("pipe-1", PIPELINE_NAME, 9876.54),
  nodes: [entity("node-1", "implementer-with-a-long-node-name", 6000), entity("node-2", "reviewer", 3876.54)],
};

const COST = {
  harnesses: HARNESSES,
  total: { ...entity("total", "Total", 9876.54), id: undefined, name: undefined },
  by_period: [],
  by_pipeline: [pipeline],
  by_model: [],
  by_project: [],
  resolved: [],
};

async function openCost(page: Page) {
  await page.setViewportSize({ width: 1024, height: 768 });
  await page.addInitScript(() => localStorage.setItem("pdo.ui.scale", "120"));
  await page.route("**/stats/cost?**", (route) => route.fulfill({ json: COST }));
  await page.goto("/");
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });
  await page.getByTestId("open-stats").click();
  await page.getByTestId("stats-tab-cost").click();
}

test("Stats › Cost at 1024 px and 120 %: the Name column stays readable, the medians whole on hover", async ({
  page,
}) => {
  await openCost(page);

  // The medians the line cuts short carry their whole text in a tooltip.
  const headline = page.getByTestId("stats-selection-headline");
  await expect(headline.locator(".sx-kpi__sub")).toHaveAttribute("title", /median per Run$/);
  for (const h of HARNESSES) {
    await expect(page.getByTestId(`stats-harness-card-${h}`).locator(".sx-kpi__sub")).toHaveAttribute(
      "title",
      /median$/,
    );
  }

  // Drill into the pipeline: its nodes, one column per harness.
  await page.getByRole("option", { name: new RegExp(PIPELINE_NAME) }).click();
  const name = page.getByRole("columnheader", { name: "Name" });
  await expect(name).toBeVisible();
  const zoom = await page.evaluate(() =>
    Number(getComputedStyle(document.documentElement).getPropertyValue("--app-zoom")) || 1,
  );
  const width = (await name.evaluate((el) => el.getBoundingClientRect().width)) / zoom;
  // 12rem at least: a node name reads, instead of the ~69 px it was squeezed to.
  expect(width).toBeGreaterThanOrEqual(180);
  // The table scrolls sideways inside its panel rather than over the next one.
  const scroller = page.locator(".sx-hscroll").filter({ has: name });
  expect(await scroller.evaluate((el) => getComputedStyle(el).overflowX)).toBe("auto");
  expect(await scroller.evaluate((el) => el.scrollWidth > el.clientWidth)).toBe(true);
  await expect(page.getByTestId("stats-detail-row").first()).toContainText("implementer-with-a-long-node-name");
  // Columns remain to the right: the edge fades until the table is scrolled to its end.
  const wrap = page.getByTestId("stats-cost-scroll");
  await expect(wrap).toHaveAttribute("data-more", "true");
  // Scrolled to the last harness, the Name column is still where it was.
  const before = await name.evaluate((el) => el.getBoundingClientRect().left);
  await scroller.evaluate((el) => el.scrollTo({ left: el.scrollWidth }));
  await expect.poll(() => scroller.evaluate((el) => el.scrollLeft)).toBeGreaterThan(0);
  await expect(wrap).toHaveAttribute("data-more", "false");
  expect(await name.evaluate((el) => el.getBoundingClientRect().left)).toBeCloseTo(before, 0);
  const firstCell = page.getByTestId("stats-detail-row").first().locator("td").first();
  expect(await firstCell.evaluate((el) => el.getBoundingClientRect().left)).toBeCloseTo(before, 0);
});
