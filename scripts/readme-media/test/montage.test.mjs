import assert from "node:assert/strict";
import { test } from "node:test";
import { chromeLayout, planCuts } from "../lib/montage.mjs";

test("keeps the windows around the markers and cuts the wait between them", () => {
  const plan = planCuts({
    keeps: [
      { start: 1000, end: 4000 },
      { start: 20_000, end: 24_000 },
    ],
    markers: [
      { name: "clicked", t: 2000 },
      { name: "answer", t: 21_000 },
    ],
    duration: 25_000,
  });
  assert.deepEqual(plan.segments, [
    { start: 1000, end: 4000, speed: 1 },
    { start: 20_000, end: 24_000, speed: 1 },
  ]);
  assert.deepEqual(plan.markers, [
    { name: "clicked", t: 1000 },
    { name: "answer", t: 4000 },
  ]);
  assert.equal(plan.durationMs, 8000, "7 s of film, frozen on the last frame up to 8 s");
  assert.equal(plan.padMs, 1000);
});

test("plays a fast-forwarded stretch at ×8, around the normal-speed windows", () => {
  const plan = planCuts({
    keeps: [
      { start: 0, end: 3000 },
      { start: 43_000, end: 48_000 },
    ],
    fasts: [{ start: 2000, end: 44_000, speed: 8 }],
    markers: [{ name: "done", t: 44_000 }],
    duration: 48_000,
  });
  assert.deepEqual(plan.segments, [
    { start: 0, end: 3000, speed: 1 },
    { start: 3000, end: 43_000, speed: 8 },
    { start: 43_000, end: 48_000, speed: 1 },
  ]);
  assert.equal(plan.durationMs, 3000 + 5000 + 5000);
  assert.deepEqual(plan.markers, [{ name: "done", t: 9000 }]);
  assert.equal(plan.padMs, 0);
});

test("merges overlapping and near windows instead of cutting a few frames", () => {
  const plan = planCuts({
    keeps: [
      { start: 0, end: 2000 },
      { start: 1500, end: 3000 },
      { start: 3200, end: 5000 },
    ],
    duration: 9000,
  });
  assert.deepEqual(plan.segments, [{ start: 0, end: 5000, speed: 1 }]);
});

test("windows are clamped to the recording", () => {
  const plan = planCuts({ keeps: [{ start: -500, end: 12_000 }], duration: 9000 });
  assert.deepEqual(plan.segments, [{ start: 0, end: 9000, speed: 1 }]);
});

test("the chrome frames a 960 px GIF around content of the crop's aspect", () => {
  const layout = chromeLayout({ gifWidth: 960, crop: { x: 0, y: 0, width: 1200, height: 760 } });
  assert.equal(layout.width, 960);
  assert.equal(layout.content.x + layout.content.width + layout.border + layout.margin, 960);
  assert.ok(Math.abs(layout.content.width / layout.content.height - 1200 / 760) < 0.01);
  assert.ok(layout.content.y >= layout.margin + layout.bar);
  assert.equal(layout.height % 2, 0);
  assert.equal(layout.content.height % 2, 0);
});
