// Records one variant of a scene: a Playwright page on the demo instance, the
// synthetic cursor, and the timeline the montage cuts (markers, kept windows,
// fast-forwarded stretches). Scenes only talk to this `ctx` — never to ffmpeg.

import { CURSOR_STYLE, cursorInitScript } from "./cursor.mjs";
import { sleep } from "./demo-instance.mjs";
import { forbiddenOnScreen } from "./screen-guard.mjs";

/** Default kept window around a marker, in ms of recording. */
const MARKER_BEFORE = 1200;
const MARKER_AFTER = 1800;
/** How often the screen guard samples the filmed DOM (lib/screen-guard.mjs). */
const SCREEN_SAMPLE_MS = 250;

export class Recorder {
  constructor({ browser, instance, variant, videoDir, gifWidth }) {
    this.browser = browser;
    this.instance = instance;
    this.variant = variant;
    this.videoDir = videoDir;
    this.gifWidth = gifWidth;
    this.markers = [];
    this.keeps = [];
    this.fasts = [];
    this.screen = [];
    this.mouse = { x: variant.viewport.width * 0.6, y: variant.viewport.height * 0.55 };
  }

  /** CSS px of cursor so it measures ~22 px in the published GIF. */
  cursorSize() {
    const cropWidth = this.variant.crop?.width ?? this.variant.viewport.width;
    return Math.round((CURSOR_STYLE.targetGifPx * cropWidth) / this.gifWidth);
  }

  async open() {
    const { viewport } = this.variant;
    this.context = await this.browser.newContext({
      viewport,
      deviceScaleFactor: 1,
      colorScheme: "dark",
      recordVideo: { dir: this.videoDir, size: viewport },
    });
    const storage = { "pdo.tour.offered": "1", ...(this.variant.localStorage ?? {}) };
    await this.context.addInitScript((entries) => {
      for (const [key, value] of Object.entries(entries)) {
        if (localStorage.getItem(key) === null) localStorage.setItem(key, typeof value === "string" ? value : JSON.stringify(value));
      }
    }, storage);
    await this.context.addInitScript(cursorInitScript, { size: this.cursorSize(), ...CURSOR_STYLE });
    this.page = await this.context.newPage();
    // The video starts on the first screencast frame, and a frame only comes
    // when the page repaints: a scene that waits before its first goto would
    // shift every cut. A dark page with a 2 px tick keeps frames coming from
    // now on, so the timeline's 0 is the video's.
    await this.page.setContent(
      '<body style="margin:0;background:#0d1117"><i style="position:fixed;left:0;top:0;width:2px;height:2px;animation:t .2s steps(2) infinite"></i>' +
        "<style>@keyframes t{from{background:#0d1117}to{background:#0e1219}}</style></body>",
    );
    this.t0 = Date.now();
    this.sampling = this.sampleScreen();
    return this.page;
  }

  /** The screen guard's loop: what the crop shows, every SCREEN_SAMPLE_MS,
   *  until close. A sample taken mid-navigation is simply skipped. */
  async sampleScreen() {
    const crop = this.variant.crop ?? { x: 0, y: 0, ...this.variant.viewport };
    while (!this.closing) {
      const t = this.now();
      try {
        const hits = await forbiddenOnScreen(this.page, { crop });
        if (hits.length > 0) this.screen.push({ t, hits });
      } catch {
        // the page is navigating (or closing): the next sample reads it
      }
      await sleep(SCREEN_SAMPLE_MS);
    }
  }

  /** ms since the video's first frame. */
  now() {
    return Date.now() - this.t0;
  }

  // ---- timeline ------------------------------------------------------------

  /** A key moment: the montage keeps `before`/`after` ms around it. */
  mark(name, { before = MARKER_BEFORE, after = MARKER_AFTER } = {}) {
    const t = this.now();
    this.markers.push({ name, t });
    this.keeps.push({ start: Math.max(0, t - before), end: t + after });
  }

  /** Keep what `fn` films, at normal speed (a gesture between two markers). */
  async keep(fn) {
    const start = this.now();
    const result = await fn();
    this.keeps.push({ start, end: this.now() });
    return result;
  }

  /** Film `fn` at `speed`× (an agent working, a page filling up). */
  async fast(fn, { speed = 8 } = {}) {
    const start = this.now();
    const result = await fn();
    this.fasts.push({ start, end: this.now(), speed });
    return result;
  }

  /** Stay still and keep it: the end state the GIF (and its poster) ends on. */
  async hold(ms) {
    const start = this.now();
    await sleep(ms);
    this.keeps.push({ start, end: this.now() });
  }

  // ---- gestures --------------------------------------------------------------

  async goto(route = "/") {
    await this.page.goto(`${this.instance.url}${route}`);
    await this.page.waitForLoadState("networkidle").catch(() => {});
    await this.page.evaluate(({ x, y }) => window.__demoCursorAt?.(x, y), this.mouse);
    await this.page.mouse.move(this.mouse.x, this.mouse.y);
  }

  async point(target) {
    if (typeof target?.x === "number") return target;
    const box = await target.boundingBox();
    if (!box) throw new Error(`cannot point at ${target}: not visible`);
    return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
  }

  /** An eased move of the real mouse, paced on the wall clock (each `move` is
   *  a round trip to the browser, so a fixed step count would drift slower). */
  async moveTo(target, { duration = 650 } = {}) {
    const to = await this.point(target);
    const from = { ...this.mouse };
    const start = Date.now();
    for (;;) {
      const k = Math.min(1, (Date.now() - start) / duration);
      const e = k < 0.5 ? 4 * k * k * k : 1 - (-2 * k + 2) ** 3 / 2;
      this.mouse = { x: from.x + (to.x - from.x) * e, y: from.y + (to.y - from.y) * e };
      await this.page.mouse.move(this.mouse.x, this.mouse.y);
      if (k === 1) return;
      await sleep(8);
    }
  }

  async click(target, { duration, pause = 140 } = {}) {
    await this.moveTo(target, { duration });
    await sleep(pause);
    await this.page.mouse.down();
    await sleep(90);
    await this.page.mouse.up();
  }

  async hover(target, { duration, pause = 500 } = {}) {
    await this.moveTo(target, { duration });
    await sleep(pause);
  }

  /** A smooth wheel scroll of `dy` px under the cursor, over `duration` ms. */
  async scroll(dy, { duration = 700 } = {}) {
    const start = Date.now();
    let done = 0;
    for (;;) {
      const k = Math.min(1, (Date.now() - start) / duration);
      const e = 1 - (1 - k) ** 3;
      const step = Math.round(dy * e) - done;
      if (step !== 0) await this.page.mouse.wheel(0, step);
      done += step;
      if (k === 1) return;
      await sleep(16);
    }
  }

  async drag(from, to, { duration = 900 } = {}) {
    await this.moveTo(from);
    await this.page.mouse.down();
    await sleep(120);
    await this.moveTo(to, { duration });
    await sleep(120);
    await this.page.mouse.up();
  }

  /** Close the page and return the recorded video with its timeline. */
  async close() {
    const video = this.page.video();
    const duration = this.now();
    this.closing = true;
    await this.sampling;
    await this.context.close();
    return { video: await video.path(), markers: this.markers, keeps: this.keeps, fasts: this.fasts, screen: this.screen, duration };
  }
}
