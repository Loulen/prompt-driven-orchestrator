// The screen guard (#883, grilling Q34): a variant fails when a demo path
// (/tmp/pdo-readme-media-*) is on screen in what its GIF keeps. Filmed for real
// (Playwright, the recorder's own sampling), then the check `record` runs on
// every variant before its montage.

import assert from "node:assert/strict";
import fs from "node:fs";
import { createRequire } from "node:module";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { sleep } from "../lib/demo-instance.mjs";
import { Recorder } from "../lib/recorder.mjs";
import { assertCleanScreen } from "../lib/screen-guard.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..", "..", "..");

let chromium = null;
try {
  ({ chromium } = createRequire(path.join(repoRoot, "frontend", "package.json"))("@playwright/test"));
} catch {
  // frontend dependencies not installed
}

const DEMO_PATH = "/tmp/pdo-readme-media-Ab12Cd/home/code/shop-app";

test("a sample in a kept window fails the variant, naming the moment and the text", () => {
  const timeline = { keeps: [{ start: 1000, end: 3000 }], fasts: [], screen: [{ t: 2000, hits: [DEMO_PATH] }] };
  assert.throws(() => assertCleanScreen(timeline), (error) => {
    assert.match(error.message, /demo path is on screen/);
    assert.match(error.message, /at 2\.0 s/);
    assert.ok(error.message.includes(DEMO_PATH));
    return true;
  });
});

test("a fast-forwarded stretch is filmed too; a cut stretch is not", () => {
  const screen = [{ t: 5000, hits: [DEMO_PATH] }];
  assert.throws(() => assertCleanScreen({ keeps: [], fasts: [{ start: 4000, end: 6000, speed: 8 }], screen }));
  assert.doesNotThrow(() => assertCleanScreen({ keeps: [{ start: 0, end: 4000 }], fasts: [], screen }));
  assert.doesNotThrow(() => assertCleanScreen({ keeps: [{ start: 0, end: 9000 }], fasts: [], screen: [] }));
});

/** Film `html` for one held second, and hand back the recorded timeline. */
async function film(browser, html, { crop, before } = {}) {
  const videoDir = fs.mkdtempSync(path.join(os.tmpdir(), "readme-media-guard-"));
  try {
    const variant = { viewport: { width: 640, height: 360 }, crop };
    const recorder = new Recorder({ browser, instance: { url: "about:blank" }, variant, videoDir, gifWidth: 640 });
    const page = await recorder.open();
    if (before) {
      await page.setContent(before);
      await sleep(600); // shown, then cut: never in the GIF
    }
    await page.setContent(`<body style="margin:0;background:#111;color:#eee;font:14px monospace">${html}</body>`);
    await recorder.hold(1000);
    return await recorder.close();
  } finally {
    fs.rmSync(videoDir, { recursive: true, force: true });
  }
}

test("filmed for real: a demo path on screen fails the scene; outside the crop, hidden or cut, it passes", { skip: chromium ? false : "run `pnpm install` in frontend/" }, async (t) => {
  const browser = await chromium.launch();
  t.after(() => browser.close());

  const shown = await film(browser, `<div style="position:absolute;left:20px;top:20px">Repo ${DEMO_PATH}</div>`);
  assert.throws(() => assertCleanScreen(shown), /demo path is on screen/);

  const typed = await film(browser, `<input style="position:absolute;left:20px;top:20px;width:500px" value="${DEMO_PATH}">`);
  assert.throws(() => assertCleanScreen(typed), /demo path is on screen/, "a form value is on screen too");

  const realistic = await film(browser, `<div style="position:absolute;left:20px;top:20px">Repo ~/code/shop-app</div>`);
  assert.doesNotThrow(() => assertCleanScreen(realistic));

  const outside = await film(browser, `<div style="position:absolute;left:20px;top:300px">Repo ${DEMO_PATH}</div>`, { crop: { x: 0, y: 0, width: 640, height: 200 } });
  assert.doesNotThrow(() => assertCleanScreen(outside), "below the crop, the GIF never shows it");

  const hidden = await film(browser, `<div style="position:absolute;left:20px;top:20px;display:none">Repo ${DEMO_PATH}</div><div title="${DEMO_PATH}">~/code/shop-app</div>`);
  assert.doesNotThrow(() => assertCleanScreen(hidden), "a hidden node or an attribute is not on screen");

  const cut = await film(browser, "<p>~/code/shop-app</p>", { before: `<p>${DEMO_PATH}</p>` });
  assert.ok(cut.screen.length > 0, "the path was sampled while it showed");
  assert.doesNotThrow(() => assertCleanScreen(cut), "shown only in a stretch the montage cuts");
});
