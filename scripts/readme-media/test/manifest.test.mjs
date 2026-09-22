import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { landVariant, readManifest, updateManifest, variantEntry } from "../lib/manifest.mjs";
import { TARGET_MS } from "../lib/montage.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..", "..", "..");

const entry = (id, durationS, markers) =>
  variantEntry({
    variant: { id, label: id, markers: ["cost-by-model", "effort"] },
    facts: { durationS, width: 960, height: 664, bytes: 1000 },
    plan: { markers },
    gif: `stats/${id}.gif`,
    poster: `stats/${id}.jpg`,
    posterBytes: 100,
    target: TARGET_MS,
  });

test("a variant entry carries duration, weight, dimensions and markers, and warns off target", () => {
  const ok = entry("a", 11.2, [
    { name: "cost-by-model", t: 4340 },
    { name: "effort", t: 8250 },
  ]);
  assert.deepEqual(ok.markers, [
    { name: "cost-by-model", t_s: 4.34 },
    { name: "effort", t_s: 8.25 },
  ]);
  assert.deepEqual(ok.warnings, []);
  assert.equal(ok.bytes, 1000);
  const long = entry("a", 18.7, [{ name: "cost-by-model", t: 1000 }]);
  assert.deepEqual(long.warnings, ["duration 18.7s outside 8–15s", "missing markers: effort"]);
});

test("recording one variant again keeps the scene's other variant and the other scenes", (t) => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "readme-media-manifest-"));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const file = path.join(dir, "manifest.json");
  updateManifest(file, "stats", { title: "Run stats by model", variants: [entry("a", 11, []), entry("b", 10, [])] });
  updateManifest(file, "hero", { title: "Hero", variants: [entry("a", 12, [])] });
  updateManifest(file, "stats", { title: "Run stats by model", variants: [entry("b", 9.5, [])] });
  const manifest = readManifest(file);
  assert.deepEqual(Object.keys(manifest.scenes), ["hero", "stats"]);
  assert.deepEqual(
    manifest.scenes.stats.variants.map((v) => [v.id, v.duration_s]),
    [
      ["a", 11],
      ["b", 9.5],
    ],
  );
});

test("a variant lands its files and its entry together; an unfinished one leaves the previous pair", (t) => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "readme-media-land-"));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const file = path.join(dir, "manifest.json");
  const work = path.join(dir, ".work", "stats", "a");
  const encode = (content) => {
    fs.mkdirSync(work, { recursive: true });
    fs.writeFileSync(path.join(work, "out.gif"), content);
    fs.writeFileSync(path.join(work, "out.jpg"), content);
  };
  const land = (durationS) =>
    landVariant({
      file,
      sceneName: "stats",
      title: "Run stats by model",
      entry: entry("a", durationS, []),
      moves: [
        [path.join(work, "out.gif"), path.join(dir, "stats", "a.gif")],
        [path.join(work, "out.jpg"), path.join(dir, "stats", "a.jpg")],
      ],
    });
  encode("run 1");
  land(13.6);
  // Run 2 is cut after encoding, before landing: the review folder and the
  // manifest still agree on run 1.
  encode("run 2");
  assert.equal(fs.readFileSync(path.join(dir, "stats", "a.gif"), "utf8"), "run 1");
  assert.equal(readManifest(file).scenes.stats.variants[0].duration_s, 13.6);
  land(11.6);
  assert.equal(fs.readFileSync(path.join(dir, "stats", "a.gif"), "utf8"), "run 2");
  assert.equal(readManifest(file).scenes.stats.variants[0].duration_s, 11.6);
  assert.equal(fs.existsSync(path.join(work, "out.gif")), false);
});

test("the review folder is ignored by git", () => {
  execFileSync("git", ["check-ignore", "-q", ".readme-media/stats/a.gif"], { cwd: repoRoot });
  execFileSync("git", ["check-ignore", "-q", ".readme-media/manifest.json"], { cwd: repoRoot });
});

// The whole seam, opt-in (it films for about a minute): `make readme-media
// SCENE=stats` → two variants, GIF + poster each, and the manifest.
test("record SCENE=stats: two variants of 8–15 s with their markers", { skip: process.env.READMEMEDIA_E2E === "1" ? false : "set READMEMEDIA_E2E=1" }, () => {
  const run = spawnSync(process.execPath, ["--disable-warning=ExperimentalWarning", path.join(here, "..", "readme-media.mjs"), "record"], {
    cwd: repoRoot,
    env: { ...process.env, SCENE: "stats" },
    encoding: "utf8",
    timeout: 10 * 60_000,
  });
  assert.equal(run.status, 0, run.stderr);
  const manifest = readManifest(path.join(repoRoot, ".readme-media", "manifest.json"));
  const variants = manifest.scenes.stats.variants;
  assert.deepEqual(
    variants.map((v) => v.id),
    ["a", "b"],
  );
  for (const v of variants) {
    assert.ok(v.duration_s >= 8 && v.duration_s <= 15, `${v.id}: ${v.duration_s}s`);
    assert.deepEqual(
      v.markers.map((m) => m.name),
      v.expected_markers,
    );
    assert.equal(v.width, 960);
    for (const file of [v.gif, v.poster]) assert.ok(fs.existsSync(path.join(repoRoot, ".readme-media", file)), file);
  }
});
