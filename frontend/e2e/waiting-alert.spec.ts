import { test, expect, type BrowserContext, type Page } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as os from "node:os";
import * as path from "node:path";
import { cleanupRuns, runMultipart } from "./helpers";

// Layer 3b — waiting alerts, the browser notification (#1011) and the sound +
// one emitting tab (#1012), ADR-0081.
//
// The browser's Notification API and WebAudio's AudioContext are replaced by
// recorders (`addInitScript`), and the page's visibility is forced: `hidden`
// unless a test says otherwise (`setVisibility` flips it and fires
// `visibilitychange`, as the browser would).
// The node `waiter` of the stub (playwright.config.ts) runs the REAL
// `pdo wait-user --message …` a second after it starts, so the daemon emits a
// genuine `node_awaiting_user` on the live `/ws`.
//
// Verifies: Settings › Notifications (permission asked, granted / refused /
// insecure states, Send a test), the notification of a live wait (title, body,
// tag), its click (Run open, waiting node selected), and silence while the tab
// is in view; then the Sound switch and its test chime, and two pages of one
// browser: one notification and one chime per transition, one chime for a
// burst, nothing while one page is in view, a closed emitter taken over.

const PIPELINE_NAME = `e2e-waiting-alert-${process.pid}-${Date.now()}`;
// Instance pipelines live under `$HOME/.pdo/pipelines` (ADR-0059), not in the
// repo: a file seeded under `<repo>/.pdo/pipelines` never reaches `POST /runs`.
const PIPELINE_DIR = path.join(os.homedir(), ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);
const WAITER_MESSAGE = "Which layout do you prefer?"; // playwright.config.ts `waiter`
const PREFS_KEY = "pdo.ui.waitingAlerts";

const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
    view: { x: 100, y: 0 }
  - id: waiter
    name: waiter
    type: agent
    isolated_worktree: false
    inputs:
      - name: in
    outputs:
      - name: out
    view: { x: 100, y: 150 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
    view: { x: 100, y: 300 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: waiter, port: in }
  - source: { node: waiter, port: out }
    target: { node: end, port: result }
`;

const createdRunIds: string[] = [];

test.beforeAll(async () => {
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await cleanupRuns(...createdRunIds);
});

interface RecorderOptions {
  permission: NotificationPermission;
  /** What `requestPermission()` answers. */
  answer?: NotificationPermission;
  visibility?: DocumentVisibilityState;
  insecure?: boolean;
  /** Pre-stored switches (this browser's localStorage). */
  prefs?: { notification: boolean; sound: boolean };
}

interface Recorded {
  title: string;
  body?: string;
  tag?: string;
}

/** One chime: the carrier frequencies its oscillators started at. */
interface RecordedChime {
  frequencies: number[];
}

const CELESTA = [1046.5, 783.99];

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

      // WebAudio recorder: each chime builds its own output chain, so every
      // `createDynamicsCompressor()` opens a chime; carriers started after it
      // (not the FM modulators, which feed a frequency param) are its notes.
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
        targets: unknown[] = [];
        connect(t: unknown) {
          this.targets.push(t);
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
        start() {
          const modulator = this.targets.some(
            (t) => t instanceof Gain && t.targets.some((x) => x instanceof Param),
          );
          if (!modulator) chimes[chimes.length - 1]?.frequencies.push(this.frequency.value);
        }
        stop() {}
      }
      class RecordingAudioContext {
        state = "running";
        currentTime = 0;
        sampleRate = 8000;
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
        createConvolver() {
          return Object.assign(new AudioNodeStub(), { buffer: null });
        }
        createDynamicsCompressor() {
          chimes.push({ frequencies: [] });
          return new AudioNodeStub();
        }
        createBuffer(channels: number, length: number) {
          const data = Array.from({ length: channels }, () => new Float32Array(length));
          return { getChannelData: (c: number) => data[c] };
        }
      }
      Object.defineProperty(window, "AudioContext", { configurable: true, value: RecordingAudioContext });
      Object.defineProperty(document, "visibilityState", {
        configurable: true,
        get: () => w.__visibility,
      });
      Object.defineProperty(document, "hidden", {
        configurable: true,
        get: () => w.__visibility !== "visible",
      });
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
    ((window as unknown as { __notifications: Recorded[] }).__notifications ?? []).map(
      ({ title, body, tag }) => ({ title, body, tag }),
    ),
  );
}

function chimes(page: Page): Promise<RecordedChime[]> {
  return page.evaluate(() => (window as unknown as { __chimes: RecordedChime[] }).__chimes ?? []);
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
  return {
    notifications: all.flatMap((a) => a.shown),
    chimes: all.flatMap((a) => a.rang),
  };
}

async function openNotificationsSettings(page: Page) {
  await page.getByTestId("open-settings").click();
  await page.getByTestId("settings-section-notifications").click();
  await expect(page.getByTestId("settings-section-body-notifications")).toBeVisible();
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

test.describe("Settings › Notifications (#1011)", () => {
  test("turning it on asks the browser; granted keeps it on; Send a test fires", async ({ page }) => {
    await installRecorder(page, { permission: "default", answer: "granted", visibility: "visible" });
    await page.goto("/");
    await openNotificationsSettings(page);
    const toggle = page.getByTestId("setting-browser-notification");
    const status = page.getByTestId("setting-browser-notification-status");
    await expect(toggle).toHaveAttribute("aria-checked", "false");
    await expect(status).toHaveText("Turning it on asks your browser for permission.");
    await expect(page.getByTestId("setting-waiting-alert-test")).toBeDisabled();

    await toggle.click();
    await expect(toggle).toHaveAttribute("aria-checked", "true");
    await expect(status).toHaveText("Allowed by this browser.");
    expect(await page.evaluate(() => (window as unknown as { __permissionRequests: number }).__permissionRequests)).toBe(1);
    expect(await page.evaluate((key) => localStorage.getItem(key), PREFS_KEY)).toContain('"notification":true');

    // The tab is in view, and the test still fires.
    await page.getByTestId("setting-waiting-alert-test").click();
    await expect.poll(() => recorded(page)).toEqual([
      expect.objectContaining({ title: "PDO — test notification" }),
    ]);
  });

  test("refused: the switch falls back off, greyed, with how to unblock", async ({ page }) => {
    await installRecorder(page, { permission: "default", answer: "denied", visibility: "visible" });
    await page.goto("/");
    await openNotificationsSettings(page);
    const toggle = page.getByTestId("setting-browser-notification");
    await toggle.click();
    const status = page.getByTestId("setting-browser-notification-status");
    await expect(status).toHaveAttribute("data-state", "denied");
    await expect(status).toContainText("Blocked by this browser for this site.");
    await expect(toggle).toHaveAttribute("aria-checked", "false");
    await expect(toggle).toBeDisabled();
  });

  test("not a secure context: greyed, names the origin", async ({ page, baseURL }) => {
    await installRecorder(page, { permission: "default", visibility: "visible", insecure: true });
    await page.goto("/");
    await openNotificationsSettings(page);
    await expect(page.getByTestId("setting-browser-notification")).toBeDisabled();
    const status = page.getByTestId("setting-browser-notification-status");
    await expect(status).toHaveAttribute("data-state", "insecure");
    await expect(status).toContainText(`You opened PDO at ${new URL(baseURL!).origin}`);
  });
});

test.describe("Waiting alert — browser notification (#1011)", () => {
  test("a live wait on a hidden tab notifies; the click opens the Run on the waiting node", async ({
    page,
    baseURL,
  }) => {
    await installRecorder(page, {
      permission: "granted",
      visibility: "hidden",
      prefs: { notification: true, sound: false },
    });
    await page.goto("/");
    await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

    const runName = `alert-${Date.now()}`;
    const runId = await launchWaiterRun(page, baseURL, runName);

    await expect.poll(() => recorded(page), { timeout: 20_000 }).toEqual([
      {
        title: `PDO — ${runName} is waiting for you`,
        body: WAITER_MESSAGE,
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
    await expect(page.getByTestId("awaiting-banner-message")).toHaveText(WAITER_MESSAGE);
  });

  test("no notification while the tab is in view", async ({ page, baseURL }) => {
    await installRecorder(page, {
      permission: "granted",
      visibility: "visible",
      prefs: { notification: true, sound: false },
    });
    await page.goto("/");
    await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });
    const runId = await launchWaiterRun(page, baseURL, `alert-visible-${Date.now()}`);
    await waitForAwaitingNode(page, baseURL, runId);
    // Give the `/ws` event the time to reach the page before asserting silence.
    await page.waitForTimeout(1_000);
    expect(await recorded(page)).toEqual([]);
  });

  test("no notification for a wait already there at load", async ({ page, baseURL }) => {
    const runId = await launchWaiterRun(page, baseURL, `alert-preexisting-${Date.now()}`);
    await waitForAwaitingNode(page, baseURL, runId);
    await installRecorder(page, {
      permission: "granted",
      visibility: "hidden",
      prefs: { notification: true, sound: false },
    });
    await page.goto("/");
    await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });
    await page.waitForTimeout(1_000);
    expect(await recorded(page)).toEqual([]);
  });
});

test.describe("Settings › Notifications — Sound (#1012)", () => {
  test("off by default; on, it needs no permission and Send a test plays the chime", async ({ page }) => {
    await installRecorder(page, { permission: "denied", visibility: "visible" });
    await page.goto("/");
    await openNotificationsSettings(page);
    const sound = page.getByTestId("setting-sound");
    await expect(sound).toHaveAttribute("aria-checked", "false");
    await expect(page.getByTestId("setting-sound-status")).toHaveText(
      "A short chime, at most once every 10 s. Needs no permission.",
    );
    // The notification is blocked; the sound is not.
    await expect(page.getByTestId("setting-browser-notification")).toBeDisabled();
    await expect(page.getByTestId("setting-waiting-alert-test")).toBeDisabled();

    await sound.click();
    await expect(sound).toHaveAttribute("aria-checked", "true");
    expect(await page.evaluate((key) => localStorage.getItem(key), PREFS_KEY)).toContain('"sound":true');
    await page.getByTestId("setting-waiting-alert-test").click();
    await expect.poll(() => chimes(page)).toEqual([{ frequencies: CELESTA }]);
    expect(await recorded(page)).toEqual([]);
  });

  test("not a secure context: the notification is greyed, and says the sound still works", async ({ page }) => {
    await installRecorder(page, { permission: "default", visibility: "visible", insecure: true });
    await page.goto("/");
    await openNotificationsSettings(page);
    await expect(page.getByTestId("setting-browser-notification-status")).toContainText("Sound still works.");
    await expect(page.getByTestId("setting-sound")).toBeEnabled();
  });

  test("the switch follows another tab of the browser", async ({ page, context }) => {
    await installRecorder(page, { permission: "default", visibility: "visible" });
    await page.goto("/");
    await openNotificationsSettings(page);
    const other = await context.newPage();
    await installRecorder(other, { permission: "default", visibility: "visible" });
    await other.goto("/");
    await openNotificationsSettings(other);
    await other.getByTestId("setting-sound").click();
    await expect(page.getByTestId("setting-sound")).toHaveAttribute("aria-checked", "true");
    await other.close();
  });
});

test.describe("Waiting alert — sound and one emitting tab (#1012)", () => {
  const BOTH_ON = { notification: true, sound: true };

  async function openTabs(context: BrowserContext, n: number) {
    const pages: Page[] = [];
    for (let i = 0; i < n; i += 1) {
      const page = await context.newPage();
      await installRecorder(page, { permission: "granted", visibility: "hidden", prefs: BOTH_ON });
      await page.goto("/");
      await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });
      pages.push(page);
    }
    // Let the tabs greet each other over the local channel.
    await pages[0].waitForTimeout(300);
    return pages;
  }

  test("two hidden tabs: exactly one notification and one chime per transition", async ({ context, baseURL }) => {
    const pages = await openTabs(context, 2);
    const runName = `alert-tabs-${Date.now()}`;
    const runId = await launchWaiterRun(pages[0], baseURL, runName);

    await expect.poll(async () => (await across(pages)).notifications.length, { timeout: 20_000 }).toBe(1);
    // Past the claim window: still one, nobody emitted twice.
    await pages[0].waitForTimeout(1_000);
    const { notifications, chimes: rang } = await across(pages);
    expect(notifications).toEqual([
      { title: `PDO — ${runName} is waiting for you`, body: WAITER_MESSAGE, tag: `${runId}:waiter:1` },
    ]);
    // A question: the Celesta.
    expect(rang).toEqual([{ frequencies: CELESTA }]);
  });

  test("a burst: two waits seconds apart → two notifications, a single chime", async ({ context, baseURL }) => {
    const pages = await openTabs(context, 2);
    await launchWaiterRun(pages[0], baseURL, `alert-burst-a-${Date.now()}`);
    await expect.poll(async () => (await across(pages)).notifications.length, { timeout: 20_000 }).toBe(1);
    await launchWaiterRun(pages[0], baseURL, `alert-burst-b-${Date.now()}`);
    await expect.poll(async () => (await across(pages)).notifications.length, { timeout: 20_000 }).toBe(2);
    await pages[0].waitForTimeout(1_000);
    const { notifications, chimes: rang } = await across(pages);
    expect(notifications).toHaveLength(2);
    expect(rang).toHaveLength(1);
  });

  test("one tab in view: no tab alerts", async ({ context, baseURL }) => {
    const pages = await openTabs(context, 2);
    await setVisibility(pages[1], "visible");
    const runId = await launchWaiterRun(pages[0], baseURL, `alert-one-visible-${Date.now()}`);
    await waitForAwaitingNode(pages[0], baseURL, runId);
    await pages[0].waitForTimeout(1_500);
    expect(await across(pages)).toEqual({ notifications: [], chimes: [] });
  });

  test("closing the tab that emitted loses no later alert", async ({ context, baseURL }) => {
    const pages = await openTabs(context, 2);
    await launchWaiterRun(pages[0], baseURL, `alert-close-a-${Date.now()}`);
    await expect.poll(async () => (await across(pages)).notifications.length, { timeout: 20_000 }).toBe(1);
    const emitter = (await recorded(pages[0])).length === 1 ? 0 : 1;
    const survivor = pages[1 - emitter];
    await pages[emitter].close();
    await survivor.waitForTimeout(300);

    await launchWaiterRun(survivor, baseURL, `alert-close-b-${Date.now()}`);
    await expect.poll(async () => (await recorded(survivor)).length, { timeout: 20_000 }).toBe(1);
  });
});
