import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import { createRequire } from "node:module";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { sleep } from "../lib/demo-instance.mjs";
import { Recorder } from "../lib/recorder.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..", "..", "..");

let chromium = null;
try {
  ({ chromium } = createRequire(path.join(repoRoot, "frontend", "package.json"))("@playwright/test"));
} catch {
  // frontend dependencies not installed
}

// The cuts are planned on the recorder's clock and applied to the video: the
// two must share their 0 even when a scene waits before its first page (a
// setup call, a pipeline to install). The video starts on the first screencast
// frame, and a static about:blank page may send none: a take once lost its
// first 2.6 s that way (intermittent, not reproduced on demand here). The
// recorder paints a ticking page at open; this guards the alignment itself.
/** The mean RGB of the video frame at `t` s. */
function colorAt(video, t) {
  const raw = execFileSync("ffmpeg", ["-v", "error", "-ss", String(t), "-i", video, "-frames:v", "1", "-vf", "scale=1:1", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"]);
  return [...raw.subarray(0, 3)];
}

test("the video and the timeline share their 0, even when the scene waits before its first page", { skip: chromium ? false : "run `pnpm install` in frontend/" }, async (t) => {
  const videoDir = fs.mkdtempSync(path.join(os.tmpdir(), "readme-media-recorder-"));
  t.after(() => fs.rmSync(videoDir, { recursive: true, force: true }));
  const browser = await chromium.launch();
  t.after(() => browser.close());
  const variant = { viewport: { width: 320, height: 200 } };
  const recorder = new Recorder({ browser, instance: { url: "about:blank" }, variant, videoDir, gifWidth: 320 });
  const page = await recorder.open();
  await sleep(2000);
  // The first page shows up at 2 s of timeline: green from then on.
  await page.setContent("<body style='margin:0;background:#10b981'></body>");
  const greenAt = recorder.now() / 1000;
  await sleep(1000);
  const { video } = await recorder.close();
  const isGreen = ([r, g, b]) => g > 150 && r < 80 && b > 90;
  assert.ok(!isGreen(colorAt(video, greenAt - 0.35)), `already green at ${greenAt - 0.35}s of video: the video starts late`);
  assert.ok(isGreen(colorAt(video, greenAt + 0.35)), `not green yet at ${greenAt + 0.35}s of video: the video starts early`);
});
