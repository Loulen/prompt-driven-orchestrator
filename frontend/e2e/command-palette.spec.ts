import { test, expect, type Page } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// S5 (CORE-112): the command field in a real browser, against the real
// embedded front. The e2e daemon is the shipped binary, with no extension:
// this spec plays the test extension of the daemon's suite on the network
// (crates/pdo/tests/test_extension/ui.js) under four ids. `test` contributes
// a global Action of `palette`, one whose Operation is refused, and a Source
// de recherche answering 200 results; `fast`, `slow` and `down` each a source
// that answers at once, after 3 s, or fails. What is under test is the front:
// its palette, the extensions' groups, the refused Action absent, a slow or
// failing source kept to its own group.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const TEST_EXTENSION = path.resolve(__dirname, "..", "..", "crates/pdo/tests/test_extension");
const BUNDLE_PATH = path.join(TEST_EXTENSION, "ui.js");
const ICON_PATH = path.join(TEST_EXTENSION, "icon.svg");

function source(id: string, name: string, search: string) {
  return {
    id,
    name,
    version: "0.0.0",
    ui: {
      bundle_url: `/ext/${id}/ui.js`,
      react_major: 19,
      ui_module_major: 0,
      locations: [],
      contributions: [{ location: "palette", id: "notes", label: "Notes", type: "search", search }],
      commands: [],
    },
  };
}

/** `GET /ext` as the daemon would answer it for the four test extensions. */
const DECLARATION = [
  {
    id: "test",
    name: "Test extension",
    version: "0.0.0",
    ui: {
      bundle_url: "/ext/test/ui.js",
      react_major: 19,
      ui_module_major: 0,
      locations: [],
      contributions: [
        { location: "topbar/end", id: "page", label: "Test page", type: "page", component: "TestPage" },
        { location: "palette", id: "new-note", label: "New note", type: "action", command: "test.new-note" },
        { location: "palette", id: "purge", label: "Purge notes", type: "action", command: "test.purge" },
        { location: "palette", id: "many", label: "Many", type: "search", search: "searchMany", icon: "/ext/test/icon.svg" },
      ],
      commands: [
        {
          id: "test.new-note",
          title: "New note",
          export: "showContext",
          operation: { resource: "test/note", verb: "create" },
        },
        {
          id: "test.purge",
          title: "Purge notes",
          export: "showContext",
          operation: { resource: "test/note", verb: "delete" },
        },
      ],
    },
  },
  source("fast", "Fast source", "searchNotes"),
  source("slow", "Slow source", "searchSlowly"),
  source("down", "Down source", "searchFailing"),
];

async function playExtensions(page: Page) {
  const bundle = await fs.readFile(BUNDLE_PATH, "utf8");
  const icon = await fs.readFile(ICON_PATH, "utf8");
  await page.route("**/ext", (route) => route.fulfill({ json: DECLARATION }));
  await page.route("**/ext/*/ui.js", (route) => route.fulfill({ body: bundle, contentType: "text/javascript" }));
  await page.route("**/ext/test/icon.svg", (route) => route.fulfill({ body: icon, contentType: "image/svg+xml" }));
  const commands: unknown[] = [];
  await page.route("**/ext/test/commands", (route) => {
    commands.push(route.request().postDataJSON());
    return route.fulfill({ json: { recorded: true } });
  });
  // The question of authorization: `test/note` × `delete` is refused.
  await page.route("**/authorize", (route) => {
    const { operations } = route.request().postDataJSON() as { operations: { resource: string; verb: unknown }[] };
    const decisions = operations.map((op) =>
      op.resource === "test/note" && op.verb === "delete"
        ? { decision: "deny", reason: "the test extension refuses delete test/note" }
        : { decision: "allow" },
    );
    return route.fulfill({ json: { decisions } });
  });
  return { commands: () => commands };
}

async function openApp(page: Page) {
  await page.goto("/");
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });
}

test("a global Action shows under its extension's name and runs; a refused one is absent", async ({ page }) => {
  const played = await playExtensions(page);
  await openApp(page);

  await page.keyboard.press("Control+k");
  const palette = page.getByTestId("command-palette");
  await expect(palette).toBeVisible();
  await expect(page.getByTestId("command-palette-input")).toBeFocused();
  const group = palette.getByTestId("palette-group-ext:test");
  await expect(group.locator(".sh-pal__head")).toHaveText("Test extension");
  await expect(group.getByText("New note")).toBeVisible();
  await expect(palette.getByText("Purge notes")).toHaveCount(0);

  // Tab stays in the box; Escape gives the focus to the command field.
  await page.keyboard.press("Tab");
  await expect(page.getByTestId("command-palette-input")).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(palette).toHaveCount(0);
  await expect(page.getByTestId("command-field")).toBeFocused();

  // `>` keeps the commands, the extensions' Actions with them; Enter runs it.
  await page.getByTestId("command-field").click();
  await page.getByTestId("command-palette-input").fill(">new note");
  await expect(palette.getByTestId("palette-group-commands").getByText("New note")).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("toast-info")).toContainText("palette");
  expect(played.commands()).toContainEqual(expect.objectContaining({ location: "palette", contribution: "new-note" }));
});

test("each source answers in its own group: at once, late, or failed with Retry", async ({ page }) => {
  await playExtensions(page);
  await openApp(page);

  await page.getByTestId("command-field").click();
  await page.getByTestId("command-palette-input").fill("search");
  const palette = page.getByTestId("command-palette");
  const fast = palette.getByTestId("palette-group-ext:fast");
  const slow = palette.getByTestId("palette-group-ext:slow");
  const down = palette.getByTestId("palette-group-ext:down");

  await expect(fast.getByTestId("palette-result")).toHaveCount(2);
  await expect(fast.getByTestId("palette-result").first()).toContainText("N-2");
  await expect(fast.getByTestId("palette-result").first()).toContainText("Search index rebuild");
  await expect(slow.getByTestId("palette-searching")).toBeVisible();
  await expect(down.getByTestId("palette-failed")).toContainText("Search failed");
  await expect(down.getByTestId("palette-failed")).toContainText("index unavailable");
  // The whole row retries: an option of the list, named for it.
  await expect(down.getByRole("option", { name: /Down source: search failed, index unavailable\. Retry/ })).toBeVisible();
  // The slow one arrives in its group; the failed one stays where it is.
  await expect(slow.getByTestId("palette-result")).toHaveCount(2, { timeout: 6_000 });
  await expect(down.getByTestId("palette-failed")).toBeVisible();
  await expect(fast.getByTestId("palette-result")).toHaveCount(2);

  // A result follows its deep link.
  await fast.getByText("Search index rebuild").click();
  await expect(palette).toHaveCount(0);
  await expect(page).toHaveURL(/#\/ext\/test\/topbar\/end\/notes\/2$/);
  await expect(page.getByTestId("test-page-path")).toHaveText("Path: notes/2");
});

test("200 results of 200 characters: capped, truncated, walked through with the keyboard", async ({ page }) => {
  await playExtensions(page);
  await openApp(page);

  await page.getByTestId("command-field").click();
  await page.getByTestId("command-palette-input").fill("cart");
  const group = page.getByTestId("palette-group-ext:test");
  await expect(group.getByTestId("palette-result")).toHaveCount(5);
  await expect(group.getByTestId("palette-more")).toHaveText("Show all 50");
  // A 200-character title ends in an ellipsis, inside the box.
  const label = group.getByTestId("palette-result").first().locator(".sh-pal__label");
  expect(await label.evaluate((el) => el.scrollWidth > el.clientWidth)).toBe(true);
  expect(await label.evaluate((el) => getComputedStyle(el).textOverflow)).toBe("ellipsis");
  const box = (await page.getByTestId("command-palette").boundingBox())!;
  const row = (await group.getByTestId("palette-result").first().boundingBox())!;
  expect(row.x + row.width).toBeLessThanOrEqual(box.x + box.width + 1);

  // « Show all »: the 50 kept; the arrows walk them and the active row stays in view.
  await group.getByTestId("palette-more").click();
  await expect(group.getByTestId("palette-result")).toHaveCount(50);
  // The click kept the focus in the field.
  const input = page.getByTestId("command-palette-input");
  await expect(input).toBeFocused();
  for (let i = 0; i < 40; i += 1) await page.keyboard.press("ArrowDown");
  const active = page.locator('.sh-pal__row[aria-selected="true"]');
  await expect(active).toHaveCount(1);
  await expect(active).toBeInViewport();
  const id = await active.getAttribute("id");
  await expect(input).toHaveAttribute("aria-activedescendant", id!);
});
