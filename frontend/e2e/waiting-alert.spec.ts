import { test, expect, type BrowserContext, type Page } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as os from "node:os";
import * as path from "node:path";
import { cleanupRuns, runMultipart } from "./helpers";

// Layer 3b — the Alerte d'attente (ADR-0100): Settings ›
// Notifications, the notification and the chime of a live wait, its click, the
// Portée d'alerte, and one emitting tab per browser.
//
// The browser's Notification API and WebAudio's AudioContext are replaced by
// recorders (`addInitScript`), and the page's visibility is forced: `hidden`
// unless a test says otherwise (`setVisibility` flips it and fires
// `visibilitychange`, as the browser would). The node `waiter` of the stub
// (playwright.config.ts) runs the REAL `pdo wait-user --message …` a second
// after it starts, so the daemon emits a genuine `node_awaiting_user` on `/ws`.

const PIPELINE_NAME = `e2e-waiting-alert-${process.pid}-${Date.now()}`;
// Instance pipelines live under `$HOME/.pdo/pipelines` (ADR-0059).
const PIPELINE_DIR = path.join(os.homedir(), ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);
const WAITER_MESSAGE = "Which layout do you prefer?"; // playwright.config.ts `waiter`
const PREFS_KEY = "pdo.ui.waitingAlerts";
/** The first note of the chime of a question (`rising`). */
const RISING_FIRST = 659.25;

const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - { name: user_prompt, side: bottom }
    view: { x: 100, y: 0 }
  - id: waiter
    name: waiter
    type: agent
    isolated_worktree: false
    inputs:
      - { name: in, side: top }
    outputs:
      - { name: out, side: bottom }
    view: { x: 100, y: 150 }
  - id: end
    name: End
    type: end
    inputs:
      - { name: result, side: top }
    view: { x: 100, y: 300 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: waiter, port: in }
  - source: { node: waiter, port: out }
    target: { node: end, port: result }
`;

const createdRunIds: string[] = [];

// A Run's first node starts in a fresh worktree, then the waiter waits a second
// before declaring its wait: the first Run of a cold daemon may take most of the
// default 30 s.
test.describe.configure({ timeout: 60_000 });

test.beforeAll(async () => {
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await cleanupRuns(...createdRunIds);
});

interface Prefs {
  notification: boolean;
  sound: boolean;
  scope?: "launched" | "projects" | "all";
  projects?: string[];
}

interface RecorderOptions {
  permission: NotificationPermission;
  /** What `requestPermission()` answers. */
  answer?: NotificationPermission;
  visibility?: DocumentVisibilityState;
  insecure?: boolean;
  /** Pre-stored preferences (this browser's localStorage). */
  prefs?: Prefs;
}

interface Recorded {
  title: string;
  body?: string;
  tag?: string;
}

async function installRecorder(page: Page, opts: RecorderOptions) {
  await page.addInitScript(
    ({ permission, answer, visibility, insecure, prefs, prefsKey }) => {
      const w = window as unknown as Record<string, unknown>;
      const shown: Array<{ title: string; body?: string; tag?: string; onclick: (() => void) | null }> = [];
      w.__notifications = shown;
      w.__visibility = visibility;
      class RecordingNotification {
        static permission: NotificationPermission = permission;
        static async requestPermission() {
          w.__permissionRequests = ((w.__permissionRequests as number) ?? 0) + 1;
          RecordingNotification.permission = answer;
          return answer;
        }
        title: string;
        body?: string;
        tag?: string;
        onclick: (() => void) | null = null;
        constructor(title: string, options: NotificationOptions = {}) {
          this.title = title;
          this.body = options.body;
          this.tag = options.tag;
          shown.push(this);
        }
        close() {}
      }
      Object.defineProperty(window, "Notification", { configurable: true, value: RecordingNotification });

      // WebAudio recorder: each chime builds its own chain, so every
      // `createDynamicsCompressor()` opens a chime; the oscillators started
      // after it are its voices.
      const chimes: Array<{ frequencies: number[] }> = [];
      w.__chimes = chimes;
      class Param {
        value = 0;
        setValueAtTime(v: number) {
          this.value = v;
          return this;
        }
        linearRampToValueAtTime(v: number) {
          this.value = v;
          return this;
        }
        exponentialRampToValueAtTime(v: number) {
          this.value = v;
          return this;
        }
      }
      class AudioNodeStub {
        connect(t: unknown) {
          return t;
        }
        disconnect() {}
      }
      class Gain extends AudioNodeStub {
        gain = new Param();
      }
      class Osc extends AudioNodeStub {
        type = "sine";
        frequency = new Param();
        onended: (() => void) | null = null;
        start() {
          chimes[chimes.length - 1]?.frequencies.push(this.frequency.value);
        }
        stop() {}
      }
      class RecordingAudioContext {
        state = "running";
        currentTime = 0;
        destination = new AudioNodeStub();
        resume() {
          return Promise.resolve();
        }
        createOscillator() {
          return new Osc();
        }
        createGain() {
          return new Gain();
        }
        createBiquadFilter() {
          return Object.assign(new AudioNodeStub(), { type: "lowpass", frequency: new Param() });
        }
        createDynamicsCompressor() {
          chimes.push({ frequencies: [] });
          return new AudioNodeStub();
        }
      }
      Object.defineProperty(window, "AudioContext", { configurable: true, value: RecordingAudioContext });
      Object.defineProperty(document, "visibilityState", { configurable: true, get: () => w.__visibility });
      Object.defineProperty(document, "hidden", { configurable: true, get: () => w.__visibility !== "visible" });
      if (insecure) Object.defineProperty(window, "isSecureContext", { configurable: true, value: false });
      if (prefs) localStorage.setItem(prefsKey, JSON.stringify(prefs));
    },
    {
      permission: opts.permission,
      answer: opts.answer ?? "granted",
      visibility: opts.visibility ?? "hidden",
      insecure: !!opts.insecure,
      prefs: opts.prefs ?? null,
      prefsKey: PREFS_KEY,
    },
  );
}

function recorded(page: Page): Promise<Recorded[]> {
  return page.evaluate(() =>
    ((window as unknown as { __notifications: Recorded[] }).__notifications ?? []).map(({ title, body, tag }) => ({
      title,
      body,
      tag,
    })),
  );
}

function chimes(page: Page): Promise<Array<{ frequencies: number[] }>> {
  return page.evaluate(() => (window as unknown as { __chimes: Array<{ frequencies: number[] }> }).__chimes ?? []);
}

async function setVisibility(page: Page, visibility: DocumentVisibilityState) {
  await page.evaluate((v) => {
    (window as unknown as { __visibility: string }).__visibility = v;
    document.dispatchEvent(new Event("visibilitychange"));
  }, visibility);
}

/** Every notification and chime, across the pages (tabs) of the browser. */
async function across(pages: Page[]) {
  const all = await Promise.all(pages.map(async (p) => ({ shown: await recorded(p), rang: await chimes(p) })));
  return { notifications: all.flatMap((a) => a.shown), chimes: all.flatMap((a) => a.rang) };
}

async function openNotificationsSettings(page: Page) {
  await page.getByTestId("open-settings").click();
  await page.getByTestId("settings-category-notifications").click();
  await expect(page.getByTestId("settings-section-body-waiting-alerts")).toBeVisible();
}

const notificationSwitch = (page: Page) => page.getByTestId("setting-row-browser-notification").getByRole("switch");
const soundSwitch = (page: Page) => page.getByTestId("setting-row-sound").getByRole("switch");

async function connected(page: Page) {
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });
}

async function launchWaiterRun(page: Page, baseURL: string | undefined, name: string): Promise<string> {
  const resp = await page.request.post(`${baseURL}/runs`, {
    multipart: runMultipart({ pipeline: PIPELINE_NAME, input: "e2e waiting alert", name }),
  });
  expect(resp.status(), await resp.text()).toBe(201);
  const { run_id } = await resp.json();
  createdRunIds.push(run_id);
  return run_id;
}

async function waitForAwaitingNode(page: Page, baseURL: string | undefined, runId: string) {
  await expect(async () => {
    const run = await (await page.request.get(`${baseURL}/runs/${runId}`)).json();
    expect(run.nodes?.waiter?.status).toBe("awaiting_user");
  }).toPass({ timeout: 20_000 });
}

test.describe("Settings › Notifications", () => {
  test("turning the notification on asks the browser; granted, it stays on and its Test fires", async ({ page }) => {
    await installRecorder(page, { permission: "default", answer: "granted", visibility: "visible" });
    await page.goto("/");
    await openNotificationsSettings(page);
    const status = page.getByTestId("setting-browser-notification-status");
    await expect(notificationSwitch(page)).toHaveAttribute("aria-checked", "false");
    await expect(status).toHaveText("Asks your browser's permission");
    await expect(page.getByTestId("setting-browser-notification-test")).toHaveAttribute("aria-disabled", "true");
    await expect(page.getByTestId("setting-row-alert-scope").getByRole("radio", { name: "Runs I launched" })).toHaveAttribute(
      "aria-checked",
      "true",
    );

    await notificationSwitch(page).click();
    await expect(notificationSwitch(page)).toHaveAttribute("aria-checked", "true");
    await expect(status).toHaveText("Allowed by this browser.");
    expect(await page.evaluate(() => (window as unknown as { __permissionRequests: number }).__permissionRequests)).toBe(1);
    expect(await page.evaluate((key) => localStorage.getItem(key), PREFS_KEY)).toContain('"notification":true');

    // The tab is in view, and the test still fires.
    await page.getByTestId("setting-browser-notification-test").click();
    await expect.poll(() => recorded(page)).toEqual([expect.objectContaining({ title: "PDO · test notification" })]);

    // The sound: no permission, its own Test, the chime of a question.
    await soundSwitch(page).click();
    await expect(soundSwitch(page)).toHaveAttribute("aria-checked", "true");
    await page.getByTestId("setting-sound-test").click();
    await expect.poll(async () => (await chimes(page)).map((c) => c.frequencies[0])).toEqual([RISING_FIRST]);
  });

  test("refused: the switch falls back off, greyed, and says how to unblock", async ({ page }) => {
    await installRecorder(page, { permission: "default", answer: "denied", visibility: "visible" });
    await page.goto("/");
    await openNotificationsSettings(page);
    await notificationSwitch(page).click();
    const status = page.getByTestId("setting-browser-notification-status");
    await expect(status.locator("[data-state]")).toHaveAttribute("data-state", "denied");
    await expect(status).toContainText("Blocked by this browser for this site.");
    await expect(notificationSwitch(page)).toHaveAttribute("aria-checked", "false");
    await expect(notificationSwitch(page)).toBeDisabled();
  });

  test("not a secure context: greyed, names the origin, the sound still works", async ({ page, baseURL }) => {
    await installRecorder(page, { permission: "default", visibility: "visible", insecure: true });
    await page.goto("/");
    await openNotificationsSettings(page);
    await expect(notificationSwitch(page)).toBeDisabled();
    const status = page.getByTestId("setting-browser-notification-status");
    await expect(status.locator("[data-state]")).toHaveAttribute("data-state", "insecure");
    await expect(status).toContainText(`You opened PDO at ${new URL(baseURL!).origin}. Sound still works.`);
    await expect(soundSwitch(page)).toBeEnabled();
  });

  test("Projects lists the Projects to check", async ({ page }) => {
    await installRecorder(page, { permission: "default", visibility: "visible" });
    await page.goto("/");
    await openNotificationsSettings(page);
    await expect(page.getByTestId("setting-row-alert-projects")).toHaveCount(0);
    await page.getByTestId("setting-row-alert-scope").getByRole("radio", { name: "Projects" }).click();
    await expect(page.getByTestId("setting-row-alert-projects")).toBeVisible();
    expect(await page.evaluate((key) => localStorage.getItem(key), PREFS_KEY)).toContain('"scope":"projects"');
  });
});

test.describe("Waiting alert — the notification", () => {
  test("a live wait on a hidden tab notifies; the click opens the Run on the waiting node", async ({ page, baseURL }) => {
    await installRecorder(page, { permission: "granted", visibility: "hidden", prefs: { notification: true, sound: false } });
    await page.goto("/");
    await connected(page);

    const runName = `alert-${Date.now()}`;
    const runId = await launchWaiterRun(page, baseURL, runName);

    await expect.poll(() => recorded(page), { timeout: 20_000 }).toEqual([
      {
        title: "waiter is waiting for you",
        body: `${runName} · Question · « ${WAITER_MESSAGE} »`,
        tag: `${runId}:waiter:1`,
      },
    ]);

    // The click: the tab comes back, the Run opens, the waiting node is selected.
    await page.evaluate(() => {
      const w = window as unknown as { __visibility: string; __notifications: Array<{ onclick: (() => void) | null }> };
      w.__visibility = "visible";
      w.__notifications[0].onclick?.();
    });
    await expect(page.getByTestId("awaiting-banner")).toBeVisible({ timeout: 10_000 });
    await expect(page.getByTestId("awaiting-banner")).toContainText(WAITER_MESSAGE);
  });

  test("no alert while the tab is in view", async ({ page, baseURL }) => {
    await installRecorder(page, { permission: "granted", visibility: "visible", prefs: { notification: true, sound: true } });
    await page.goto("/");
    await connected(page);
    const runId = await launchWaiterRun(page, baseURL, `alert-visible-${Date.now()}`);
    await waitForAwaitingNode(page, baseURL, runId);
    // Give the `/ws` event the time to reach the page before asserting silence.
    await page.waitForTimeout(1_000);
    expect(await across([page])).toEqual({ notifications: [], chimes: [] });
  });

  test("no alert for a wait already there at load", async ({ page, baseURL }) => {
    const runId = await launchWaiterRun(page, baseURL, `alert-preexisting-${Date.now()}`);
    await waitForAwaitingNode(page, baseURL, runId);
    await installRecorder(page, { permission: "granted", visibility: "hidden", prefs: { notification: true, sound: true } });
    await page.goto("/");
    await connected(page);
    await page.waitForTimeout(1_000);
    expect(await across([page])).toEqual({ notifications: [], chimes: [] });
  });

  test("a Run out of the Portée: Projects with none checked, no alert", async ({ page, baseURL }) => {
    await installRecorder(page, {
      permission: "granted",
      visibility: "hidden",
      prefs: { notification: true, sound: true, scope: "projects", projects: [] },
    });
    await page.goto("/");
    await connected(page);
    const runId = await launchWaiterRun(page, baseURL, `alert-out-of-scope-${Date.now()}`);
    await waitForAwaitingNode(page, baseURL, runId);
    await page.waitForTimeout(1_500);
    expect(await across([page])).toEqual({ notifications: [], chimes: [] });
  });
});

test.describe("Waiting alert — one emitting tab", () => {
  const BOTH_ON = { notification: true, sound: true };

  async function openTabs(context: BrowserContext, n: number) {
    const pages: Page[] = [];
    for (let i = 0; i < n; i += 1) {
      const page = await context.newPage();
      await installRecorder(page, { permission: "granted", visibility: "hidden", prefs: BOTH_ON });
      await page.goto("/");
      await connected(page);
      pages.push(page);
    }
    // Let the tabs greet each other over the local channel.
    await pages[0].waitForTimeout(300);
    return pages;
  }

  test("two hidden tabs: exactly one notification and one chime", async ({ context, baseURL }) => {
    const pages = await openTabs(context, 2);
    const runName = `alert-tabs-${Date.now()}`;
    const runId = await launchWaiterRun(pages[0], baseURL, runName);

    await expect.poll(async () => (await across(pages)).notifications.length, { timeout: 20_000 }).toBe(1);
    // Past the claim window: still one, nobody emitted twice.
    await pages[0].waitForTimeout(1_000);
    const { notifications, chimes: rang } = await across(pages);
    expect(notifications).toEqual([
      { title: "waiter is waiting for you", body: `${runName} · Question · « ${WAITER_MESSAGE} »`, tag: `${runId}:waiter:1` },
    ]);
    expect(rang.map((c) => c.frequencies[0])).toEqual([RISING_FIRST]);
  });

  test("one tab in view: no tab alerts", async ({ context, baseURL }) => {
    const pages = await openTabs(context, 2);
    await setVisibility(pages[1], "visible");
    const runId = await launchWaiterRun(pages[0], baseURL, `alert-one-visible-${Date.now()}`);
    await waitForAwaitingNode(pages[0], baseURL, runId);
    await pages[0].waitForTimeout(1_500);
    expect(await across(pages)).toEqual({ notifications: [], chimes: [] });
  });
});
