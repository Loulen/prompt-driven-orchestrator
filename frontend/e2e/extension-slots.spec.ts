import { test, expect, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import * as fs from "node:fs/promises";
import * as os from "node:os";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { cleanupRuns, runMultipart, E2E_TARGET_REPO } from "./helpers";

// Stage 5 (ADR-0080) — #931, #29: the UI locations (Emplacements d'UI) in a real
// browser, against the real embedded front.
//
// The e2e daemon is the shipped binary, with no extension: this spec plays the
// test extension of the daemon's suite (crates/pdo/tests/test_extension/) on the
// network — `GET /ext` declares it, `/ext/test/ui.js` serves its very bundle —
// and adds its attribute to one real Run. What is under test is the front: it
// reads the declarations, imports the ES bundle at runtime with React shared
// through the import map, renders each slot, and refuses a bundle built for
// another React major with a banner.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const BUNDLE_PATH = path.join(WORKSPACE_ROOT, "crates/pdo/tests/test_extension/ui.js");
const ICON_PATH = path.join(WORKSPACE_ROOT, "crates/pdo/tests/test_extension/icon.svg");
const INBOX_ICON_PATH = path.join(WORKSPACE_ROOT, "crates/pdo/tests/test_extension/inbox.svg");
const PIPELINE_NAME = `e2e-extension-slots-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(os.homedir(), ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);
const REACT_MAJOR = 19;
const HOST_A_PATH = path.join(WORKSPACE_ROOT, "crates/pdo/tests/test_extension/host_a.js");
const CONTRIBUTOR_B_PATH = path.join(WORKSPACE_ROOT, "crates/pdo/tests/test_extension/contributor_b.js");

const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - { name: user_prompt, side: bottom }
    view: { x: 100, y: 0 }
  - id: worker
    name: worker
    type: agent
    isolated_worktree: false
    inputs:
      - { name: task, side: top }
    outputs:
      - { name: summary, side: bottom }
    view: { x: 100, y: 150 }
  - id: end
    name: End
    type: end
    inputs:
      - { name: result, side: top }
    view: { x: 100, y: 300 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: summary }
    target: { node: end, port: result }
`;

/** The Action locations (#30), in the test extension's declaration order (`ACTION_LOCATIONS`). */
const ACTION_LOCATIONS = [
  "runs/header",
  "run/item/menu",
  "runs/bulk",
  "run/toolbar",
  "canvas/node/menu",
  "canvas/edge/menu",
  "node/toolbar",
  "trigger/item/menu",
  "pipeline/item/menu",
  "pipelines/header",
  "tab/menu",
  "review/toolbar",
  "review/file",
  "skill/menu",
];

/** The id of the test extension's action at `location` (`action_id`). */
const actionId = (location: string) => `at-${location.replaceAll("/", "-")}`;

/** Its action at each Action location, then the command that fails, and their commands. */
function actionContributions() {
  const contributions = ACTION_LOCATIONS.map((location) => ({
    location,
    id: actionId(location),
    label: `Test ${location}`,
    type: "action",
    command: `test.${actionId(location)}`,
    ...(location === "run/item/menu" ? { when: { not: { eq: ["entity.status", "archived"] } } } : {}),
  }));
  contributions.push({ location: "run/item/menu", id: "fail", label: "Fail on purpose", type: "action", command: "test.fail" });
  const commands = ACTION_LOCATIONS.map((location) => ({
    id: `test.${actionId(location)}`,
    title: `Test ${location}`,
    export: "showContext",
  }));
  commands.push({ id: "test.fail", title: "Fail on purpose", export: "failOnPurpose" });
  return { contributions, commands };
}

/** `GET /ext` as the daemon answers it for the test extension (`extension_hooks.rs`). */
function declaration(reactMajor: number) {
  const icon = "/ext/test/icon.svg";
  const actions = actionContributions();
  return [
    {
      id: "test",
      name: "Test extension",
      version: "0.0.0",
      ui: {
        bundle_url: "/ext/test/ui.js",
        react_major: reactMajor,
        ui_module_major: null,
        locations: [],
        contributions: [
          // Its four 0.1 slots, mapped onto their locations.
          { location: "topbar/start", id: "events", label: "Test events", type: "page", component: "EventsPage" },
          { location: "settings", id: "settings", label: "Test extension", type: "section", component: "SettingsPanel" },
          { location: "runs/filter", id: "started-by", label: "Started by", type: "list_filter", component: "StartedByFilter" },
          { location: "run/detail", id: "run-tab", label: "Test", type: "tab", component: "RunTab" },
          // Its 0.2 contributions to `topbar/end`.
          { location: "topbar/end", id: "page", label: "Test page", type: "page", component: "TestPage", icon },
          // 0.20: a panel under its icon, which counts the inbox's items.
          {
            location: "topbar/end", id: "inbox", label: "Test inbox", type: "panel", component: "InboxPanel",
            icon: "/ext/test/inbox.svg", counter: "/ext/test/inbox/counter",
          },
          {
            location: "topbar/end", id: "show-context", label: "Show context", type: "action",
            command: "test.show-context", when: { has: "run" },
          },
          {
            location: "topbar/end", id: "never", label: "Never shown", type: "action",
            command: "test.never", when: { eq: ["run.status", "never-a-status"] },
          },
          ...actions.contributions,
        ],
        commands: [
          { id: "test.show-context", title: "Show context", export: "showContext" },
          { id: "test.never", title: "Never shown", export: "showContext" },
          ...actions.commands,
        ],
      },
    },
  ];
}

const createdRuns: string[] = [];
let runId = "";

test.beforeAll(async () => {
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async () => {
  await cleanupRuns(...createdRuns);
  await fs.rm(PIPELINE_PATH, { force: true });
  // The daemon never deletes a Run's worktree nor its branch (ADR-0012): the
  // spec takes back what its Run left in the workspace's repository.
  for (const id of createdRuns) {
    const git = (args: string[]) => {
      try {
        execFileSync("git", args, { cwd: E2E_TARGET_REPO, stdio: "ignore" });
      } catch {
        // best-effort teardown
      }
    };
    git(["worktree", "remove", "--force", path.join(E2E_TARGET_REPO, ".pdo", "runs", id, "worktree")]);
    git(["branch", "-D", `pdo/run-${id}`]);
  }
});

/** Play the test extension on the network; the Run `runId` carries its attribute. */
async function playTestExtension(page: Page, reactMajor = REACT_MAJOR) {
  const bundle = await fs.readFile(BUNDLE_PATH, "utf8");
  let bundleRequests = 0;
  await page.route("**/ext", (route) => route.fulfill({ json: declaration(reactMajor) }));
  await page.route("**/ext/test/ui.js", (route) => {
    bundleRequests += 1;
    return route.fulfill({ body: bundle, contentType: "text/javascript" });
  });
  const icon = await fs.readFile(ICON_PATH, "utf8");
  await page.route("**/ext/test/icon.svg", (route) =>
    route.fulfill({ body: icon, contentType: "image/svg+xml" }),
  );
  const inboxIcon = await fs.readFile(INBOX_ICON_PATH, "utf8");
  await page.route("**/ext/test/inbox.svg", (route) =>
    route.fulfill({ body: inboxIcon, contentType: "image/svg+xml" }),
  );
  // The inbox of `inbox.rs`, played the same way: three items, deleted and restored.
  const inbox = [
    { id: "item-1", title: "Review the release notes", deleted: false },
    { id: "item-2", title: "Answer the question", deleted: false },
    { id: "item-3", title: "Plan the demo", deleted: false },
  ];
  const live = () => inbox.filter((item) => !item.deleted).map(({ id, title }) => ({ id, title }));
  await page.route("**/ext/test/inbox/counter", (route) =>
    route.fulfill({ json: { count: live().length, accent: false, detail: `${live().length} items` } }),
  );
  await page.route(/\/ext\/test\/inbox(\/[^/]+(\/restore)?)?$/, (route) => {
    if (route.request().url().endsWith("/counter")) return route.fallback();
    const match = /\/inbox\/([^/]+)/.exec(route.request().url());
    const item = inbox.find((i) => i.id === match?.[1]);
    if (item) item.deleted = route.request().method() === "DELETE";
    return route.fulfill({ json: live() });
  });
  const commands: unknown[] = [];
  await page.route("**/ext/test/commands", (route) => {
    commands.push(route.request().postDataJSON());
    return route.fulfill({ json: { recorded: true } });
  });
  await page.route("**/ext/test/events", (route) =>
    route.fulfill({
      json: [
        {
          type: "operation_applied",
          actor: { kind: "local_operator" },
          operation: { resource: "instance_settings", verb: "update", target: null },
          entity: null,
        },
        { type: "event", event: { kind: "RunStarted", run_id: runId } },
      ],
    }),
  );
  await page.route("**/ext/test", (route) =>
    route.fulfill({ json: { id: "test", data_dir: "/home/pdo/.pdo/extensions/test" } }),
  );
  const attributes = { "test/started_by": "alice" };
  await page.route("**/runs", async (route) => {
    if (route.request().method() !== "GET") return route.continue();
    const response = await route.fetch();
    const runs = (await response.json()) as { run_id: string; attributes?: object }[];
    for (const run of runs) if (run.run_id === runId) run.attributes = attributes;
    return route.fulfill({ response, json: runs });
  });
  await page.route(`**/runs/${runId}`, async (route) => {
    if (route.request().method() !== "GET") return route.continue();
    const response = await route.fetch();
    return route.fulfill({ response, json: { ...(await response.json()), attributes } });
  });
  return { bundleRequests: () => bundleRequests, commands: () => commands };
}

test.beforeEach(async ({ page, baseURL }) => {
  if (runId) return;
  const resp = await page.request.post(`${baseURL}/runs`, {
    multipart: runMultipart({ pipeline: PIPELINE_NAME, input: "extension slots" }),
  });
  expect(resp.status()).toBe(201);
  runId = (await resp.json()).run_id;
  createdRuns.push(runId);
});

test("the four slots show the test extension's contributions", async ({ page }) => {
  // Four surfaces in a row, each read through the network: past the default 30 s on a loaded machine.
  test.setTimeout(90_000);
  const played = await playTestExtension(page);
  await page.goto("/");
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  // Navigation entry → its page lists the OperationApplied and the Run's events.
  const entry = page.getByTestId("extension-nav-test:events");
  await expect(entry).toHaveText("Test events");
  expect(played.bundleRequests()).toBe(1);
  await entry.click();
  const events = page.getByTestId("test-extension-events");
  await expect(events).toContainText("OperationApplied: update instance_settings");
  await expect(events).toContainText(`RunStarted (${runId})`);
  await page.getByRole("button", { name: "Close Test events" }).click();

  // List filter, a row of the funnel's menu → only the Run carrying the attribute stays.
  await page.getByTestId("run-filter-funnel").click();
  const filter = page.getByLabel("Started by");
  await expect(filter).toBeVisible();
  await filter.fill("alice");
  await expect(page.getByText(runId.slice(0, 20)).first()).toBeVisible();
  await filter.fill("bob");
  await expect(page.getByTestId("run-filter-empty")).toBeVisible();
  await filter.fill("");
  await page.keyboard.press("Escape");

  // Detail tab of the Run → its attributes.
  await page.getByText(runId.slice(0, 20)).first().click({ position: { x: 5, y: 5 } });
  await page.getByTestId("toolbar-info").click();
  // A narrow strip puts the extension's tab behind the « More tabs » chevron.
  const runTab = page.getByTestId("info-tab-ext-test:run-tab");
  const more = page.getByTestId("info-tabs-more");
  await expect(page.locator('[data-testid="info-tab-ext-test:run-tab"]:not([hidden]), [data-testid="info-tabs-more"]').first()).toBeVisible();
  if (await runTab.isVisible()) {
    await runTab.click();
  } else {
    await more.click();
    await page.getByRole("menuitem", { name: "Test", exact: true }).click();
  }
  await expect(page.getByTestId("test-extension-run-tab")).toContainText(
    "test/started_by = alice",
  );

  // Settings panel, on its own rail entry.
  await page.getByTestId("open-settings").click();
  await page.getByTestId("settings-category-extensions").click();
  await expect(page.getByTestId("test-extension-settings")).toContainText(
    "Data folder: /home/pdo/.pdo/extensions/test",
  );
});

test("topbar/end: an icon before Stats, its page with the context, a command, a deep link", async ({
  page,
}) => {
  const played = await playTestExtension(page);
  await page.goto("/");
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  // The icon sits in the right-hand group, before Stats; the action whose
  // `when` never holds is not there, the one that needs a Run neither (yet).
  const icon = page.getByTestId("extension-topbar-end-test:page");
  await expect(icon).toBeVisible();
  const order = await page
    .locator("header .sh-tb__end button")
    .evaluateAll((buttons) => buttons.map((b) => b.getAttribute("data-testid")));
  expect(order.indexOf("extension-topbar-end-test:page")).toBeLessThan(order.indexOf("open-stats"));
  await expect(page.getByTestId("extension-topbar-end-test:never")).toHaveCount(0);
  await expect(page.getByTestId("extension-topbar-end-test:show-context")).toHaveCount(0);

  // Select the Run, open the page: it shows the Run.
  await page.getByText(runId.slice(0, 20)).first().click({ position: { x: 5, y: 5 } });
  await icon.click();
  await expect(page.getByTestId("test-page-run")).toContainText(`Run ${runId}`);
  await expect(page).toHaveURL(/#\/ext\/test\/topbar\/end$/);
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("extension-page")).toHaveCount(0);

  // The command receives the Run of its location's context.
  await page.getByTestId("extension-topbar-end-test:show-context").click();
  await expect(page.getByTestId("toast-info")).toContainText(`Run ${runId}`);
  expect(played.commands()).toContainEqual(expect.objectContaining({ location: "topbar/end" }));

  // A deep link opens the page on its path; one toward an absent extension says so.
  await page.goto("/#/ext/test/topbar/end/deep/path");
  await expect(page.getByTestId("test-page-path")).toHaveText("Path: deep/path");
  // Its socket, opened through `openProbedSocket` of `@pdo/extension-ui`
  // (ADR-0083), is open and receives the stream (`ready`, then heartbeats).
  await expect(page.getByTestId("test-page-socket")).toHaveText("Socket: ouvert (receiving)", { timeout: 10_000 });
  await page.goto("/#/ext/audit/journal");
  await expect(page.getByTestId("toast-error")).toContainText("Extension “audit” is not installed");
});

test("topbar/end: a panel under its counted icon, Undo in a toast keeps it open", async ({ page }) => {
  await playTestExtension(page);
  await page.goto("/");
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  // The icon counts the three items, and opens the panel right under it.
  const icon = page.getByTestId("extension-topbar-end-test:inbox");
  await expect(page.getByTestId("extension-topbar-end-count-test:inbox")).toHaveText("3");
  await expect(icon).toHaveAccessibleName("Test inbox, 3");
  await icon.click();
  const panel = page.getByRole("dialog", { name: "Test inbox" });
  await expect(panel).toBeVisible();
  const [iconBox, panelBox] = [await icon.boundingBox(), await panel.boundingBox()];
  expect(panelBox!.y).toBeGreaterThanOrEqual(iconBox!.y + iconBox!.height);
  expect(Math.abs(panelBox!.x + panelBox!.width - (iconBox!.x + iconBox!.width))).toBeLessThanOrEqual(1);

  // Delete an item, then Undo in the toast: the panel stays open, the item comes back.
  await panel.getByRole("button", { name: "Delete Answer the question" }).click();
  await expect(panel.getByTestId("test-inbox-item-2")).toHaveCount(0);
  await page.getByTestId("toast-info").getByRole("button", { name: "Undo" }).click();
  await expect(panel).toBeVisible();
  await expect(panel.getByTestId("test-inbox-item-2")).toBeVisible();

  // Escape closes it, the focus back on the icon; a click outside too.
  await page.keyboard.press("Escape");
  await expect(panel).toHaveCount(0);
  await expect(icon).toBeFocused();
  await icon.click();
  await expect(panel).toBeVisible();
  await page.mouse.click(10, 300);
  await expect(panel).toHaveCount(0);
});

test("a full-window page names its header per route, a Banner shows a lock, and yjs loads once", async ({ page }) => {
  const logs: string[] = [];
  page.on("console", (message) => logs.push(message.text()));
  await playTestExtension(page);
  await page.goto("/#/ext/test/topbar/end/screens/alpha");
  const header = page.getByTestId("extension-page-header");
  await expect(header.getByRole("heading")).toHaveText("Alpha screen");
  await expect(header.getByTestId("extension-page-icon").locator("svg")).toBeVisible();
  await page.getByRole("button", { name: "Go to Beta screen" }).click();
  await expect(header.getByRole("heading")).toHaveText("Beta screen");
  await expect(header.getByTestId("extension-page-icon").getByTestId("extension-icon")).toBeVisible();
  // The read-only state of the Beta screen: a lock, in the Banner's tone.
  const banner = page.getByRole("status").filter({ hasText: "Read-only" });
  await expect(banner.locator("svg.d-banner__icon")).toBeVisible();

  // Its root sets nothing: the page's label and icon are back.
  await page.goto("/#/ext/test/topbar/end");
  await expect(header.getByRole("heading")).toHaveText("Test page");
  // The page's yjs document, made with the front's own yjs.
  await expect(page.getByTestId("test-page-yjs")).toHaveText("Yjs: shared");
  const shared = await page.evaluate(async () => {
    const shim = await import(/* @vite-ignore */ "/shared/yjs.js" as string);
    return shim.Doc === (globalThis as { __PDO_SHARED__?: { yjs: { Doc: unknown } } }).__PDO_SHARED__?.yjs.Doc;
  });
  expect(shared).toBe(true);
  expect(logs.filter((line) => line.includes("Yjs was already imported"))).toEqual([]);
});

test("Action locations: the header, a row's « ⋯ » menu with its when, a failing command, the bulk bar", async ({
  page,
}) => {
  const played = await playTestExtension(page);
  await page.goto("/");
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  // `runs/header`: its icon button, before « New Run ».
  await page.getByTestId("extension-action-test:at-runs-header").click();
  await expect(page.getByTestId("toast-info").first()).toContainText("runs/header");

  // `run/item/menu`: the row's menu lists the core's verbs, the group titled
  // « Test extension », then the destructive entry; the command gets the row's Run.
  const row = page.locator(`[data-run-row="${runId}"]`);
  await row.hover();
  await row.getByTestId("run-row-menu").click();
  const group = page.getByTestId("extension-menu-run/item/menu-test");
  await expect(group).toContainText("Test extension");
  await page.getByTestId("extension-action-test:at-run-item-menu").click();
  await expect(page.getByTestId("toast-info").last()).toContainText(`run/item/menu · run ${runId}`);
  expect(played.commands()).toContainEqual(
    expect.objectContaining({ location: "run/item/menu", entity: expect.objectContaining({ id: runId }) }),
  );

  // A failing command: an error toast, and the menu still works.
  await row.hover();
  await row.getByTestId("run-row-menu").click();
  await page.getByTestId("extension-action-test:fail").click();
  await expect(page.getByTestId("toast-error")).toContainText("the test extension fails on purpose");
  await row.hover();
  await row.getByTestId("run-row-menu").click();
  await expect(page.getByTestId("run-menu-copy-id")).toBeVisible();
  await page.keyboard.press("Escape");

  // `runs/bulk`: the selection's ids reach the command.
  // The status dot is the row's checkbox (#577): hidden on hover, the ring
  // takes its place.
  await row.hover();
  await row.getByRole("checkbox", { name: /^Select / }).click();
  await page.getByTestId("bulk-action-test:at-runs-bulk").click();
  expect(played.commands()).toContainEqual(
    expect.objectContaining({ location: "runs/bulk", selection: { kind: "run", ids: [runId] } }),
  );
  // The toasts stack over the bottom of the window, the bulk bar's end included.
  const dismiss = page.getByRole("button", { name: "Dismiss" });
  while ((await dismiss.count()) > 0) await dismiss.first().click();
  await page.getByTestId("bulk-clear").click();
});

test("a bundle built for another React major is not loaded, and a banner says so", async ({
  page,
}) => {
  const played = await playTestExtension(page, REACT_MAJOR - 1);
  await page.goto("/");
  await expect(page.getByTestId("extension-banner-test")).toContainText(
    `built for React ${REACT_MAJOR - 1}, and the app runs React ${REACT_MAJOR}`,
  );
  expect(played.bundleRequests()).toBe(0);
  await expect(page.getByTestId("extension-nav")).toHaveCount(0);
  // The rest of the UI works.
  await page.getByTestId("open-settings").click();
  await expect(page.getByTestId("settings-surface")).toBeVisible();
  await expect(page.getByTestId("settings-category-extensions")).toHaveCount(0);
});

/** `GET /ext` for A and B of `test_extension/hosted.rs`, B with `panels` sections. */
function hostedDeclaration(panels: number, withB = true) {
  const bundle = (id: string) => ({ bundle_url: `/ext/${id}/ui.js`, react_major: REACT_MAJOR });
  const panel = (id: string, label: string, component: string) =>
    ({ location: "a/panneau", id, label, type: "section", component });
  const a = {
    id: "a",
    name: "Test host A",
    version: "0.0.0",
    ui: {
      ...bundle("a"),
      ui_module_major: 0,
      contributions: [{ location: "topbar/end", id: "host", label: "Host A", type: "page", component: "HostPage" }],
      commands: [],
      locations: [
        { id: "a/panneau", accepts: "section", description: "", context: [{ key: "item", description: "" }] },
        { id: "a/actions", accepts: "action", description: "", context: [{ key: "item", description: "" }] },
      ],
    },
  };
  const b = {
    id: "b",
    name: "Test contributor B",
    version: "0.0.0",
    ui: {
      ...bundle("b"),
      ui_module_major: null,
      contributions: [
        panel("panel", "B panel", "BPanel"),
        ...(panels >= 2 ? [panel("second-panel", "B second panel", "BSecondPanel")] : []),
        { location: "a/actions", id: "ping", label: "Ping from B", type: "action", command: "b.ping" },
      ],
      commands: [{ id: "b.ping", title: "Ping from B", export: "ping" }],
      locations: [],
    },
  };
  return withB ? [b, a] : [a];
}

async function playHosted(page: Page, declaration: unknown[]) {
  const hostA = await fs.readFile(HOST_A_PATH, "utf8");
  const contributorB = await fs.readFile(CONTRIBUTOR_B_PATH, "utf8");
  await page.route("**/ext", (route) => route.fulfill({ json: declaration }));
  await page.route("**/ext/a/ui.js", (route) => route.fulfill({ body: hostA, contentType: "text/javascript" }));
  await page.route("**/ext/b/ui.js", (route) =>
    route.fulfill({ body: contributorB, contentType: "text/javascript" }),
  );
}

test("hosted locations: A renders B's contributions through @pdo/extension-ui", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await playHosted(page, hostedDeclaration(1));
  await page.goto("/#/ext/a/topbar/end");
  // B's section under each of A's items, with the context A handed it.
  await expect(page.getByTestId("b-panel-item-1")).toHaveText("b panel · a/panneau · First item");
  await expect(page.getByTestId("b-panel-item-2")).toHaveText("b panel · a/panneau · Second item");
  await expect(page.getByTestId("host-a-item-1-count")).toHaveText("Contributions: 1");
  // B's action in A's header calls B's command with A's context.
  await page.getByTestId("hosted-action-b:ping").click();
  await expect(page.getByTestId("toast-info")).toContainText("B pinged from a/actions on First item");

  // Two sections of B: both show.
  await page.unrouteAll();
  await playHosted(page, hostedDeclaration(2));
  await page.reload();
  await expect(page.getByTestId("b-second-panel-item-1")).toBeVisible();
  await expect(page.getByTestId("b-panel-item-1")).toBeVisible();
  await expect(page.getByTestId("host-a-item-1-count")).toHaveText("Contributions: 2");

  // B removed: A's page shows without its elements.
  await page.unrouteAll();
  await playHosted(page, hostedDeclaration(1, false));
  await page.reload();
  await expect(page.getByTestId("host-a-item-1-count")).toHaveText("Contributions: 0");
  await expect(page.getByTestId("b-panel-item-1")).toHaveCount(0);
  await expect(page.getByTestId("host-a-actions")).toBeEmpty();
  expect(errors).toEqual([]);
});

test("without extension, no element is added", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });
  await expect(page.getByTestId("extension-nav")).toHaveCount(0);
  await expect(page.getByTestId("extension-banner")).toHaveCount(0);
  await expect(page.getByTestId("extension-filters-run")).toHaveCount(0);
});
