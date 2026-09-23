// The three settings scenes of #857 (triggers, profiles, skills): what they
// declare, and the fixtures they film — the guard script whose dry-run the
// Triggers GIF shows, and the local skills repo the Skill bank GIF imports.
// The full recording is opt-in (READMEMEDIA_E2E=1), like the Stats one.

import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { planHistory } from "../lib/history-plan.mjs";
import { readManifest } from "../lib/manifest.mjs";
import { loadScenes } from "../lib/scenes.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..", "..", "..");
const fixture = path.join(here, "..", "fixture");
const MARKERS = {
  triggers: [
    ["guard-tested", "history-shown"],
    ["guard-tested", "history-shown"],
  ],
  profiles: [
    ["profile-changed", "nodes-switched"],
    ["profile-changed", "nodes-switched"],
  ],
  skills: [
    ["skill-created", "import", "skill-added"],
    ["import", "skill-added"],
  ],
};

test("triggers, profiles and skills: two variants each, their markers, no live agent", async () => {
  const scenes = await loadScenes(path.join(here, "..", "scenes"));
  for (const [name, markers] of Object.entries(MARKERS)) {
    const scene = scenes.get(name);
    assert.ok(scene, `scene ${name}`);
    assert.deepEqual(
      scene.variants.map((v) => v.id),
      ["a", "b"],
    );
    assert.deepEqual(
      scene.variants.map((v) => v.markers),
      markers,
    );
    assert.deepEqual(scene.live, [], `${name} plays no live harness (no auth staged)`);
  }
});

test("the guard exits 0 on a degraded prod, its report is the incident runs' input", () => {
  const run = spawnSync(path.join(fixture, "shop-app", "prod-health-check.sh"), { encoding: "utf8", cwd: os.tmpdir() });
  assert.equal(run.status, 0, run.stderr);
  // The same report the mocked history's fired entries carry as guard stdout.
  const plan = planHistory({ now: new Date("2026-09-22T12:00:00Z"), targetRepo: "/tmp/shop-app", triggerId: "trg-demo" });
  const fired = plan.fires.filter((f) => f.outcome === "fired");
  assert.ok(fired.length > 0);
  for (const fire of fired) assert.equal(run.stdout.trim(), fire.guard_stdout);
});

test("the guard exits 1 on a healthy prod: the minute is skipped", (t) => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "readme-media-guard-"));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  fs.cpSync(path.join(fixture, "shop-app"), dir, { recursive: true });
  const probe = path.join(dir, "ops", "prod-probe.env");
  fs.writeFileSync(probe, fs.readFileSync(probe, "utf8").replace(/^P95_MS=.*$/m, "P95_MS=310").replace(/^ERROR_RATE_PCT=.*$/m, "ERROR_RATE_PCT=0"));
  const run = spawnSync(path.join(dir, "prod-health-check.sh"), { encoding: "utf8" });
  assert.equal(run.status, 1);
  assert.match(run.stdout, /^OK — checkout p95 310 ms/);
});

test("the guard is executable in the fixture's git index (the demo repo is a fresh copy)", () => {
  const mode = execFileSync("git", ["ls-files", "--stage", "scripts/readme-media/fixture/shop-app/prod-health-check.sh"], { cwd: repoRoot, encoding: "utf8" });
  // Untracked while the change is being written: the file mode then speaks.
  if (mode.trim()) assert.match(mode, /^100755 /);
  else assert.ok(fs.statSync(path.join(fixture, "shop-app", "prod-health-check.sh")).mode & 0o111);
});

test("the local skills repo holds valid SKILL.md files, one per folder, named after it", () => {
  const root = path.join(fixture, "qa-skills", "skills");
  const names = fs.readdirSync(root).sort();
  assert.deepEqual(names, ["a11y-audit", "annotate-screenshots", "playwright-capture", "visual-diff"]);
  for (const name of names) {
    const md = fs.readFileSync(path.join(root, name, "SKILL.md"), "utf8");
    const front = md.match(/^---\n([\s\S]*?)\n---\n([\s\S]+)$/);
    assert.ok(front, `${name}: frontmatter + body`);
    assert.match(front[1], new RegExp(`^name: ${name}$`, "m"));
    assert.match(front[1], /^description: \S.+$/m);
    assert.ok(front[2].trim().length > 0, `${name}: body`);
  }
});

test(
  "record SCENE=triggers,profiles,skills: six variants of 8–15 s, their markers, nothing left running",
  { skip: process.env.READMEMEDIA_E2E === "1" ? false : "set READMEMEDIA_E2E=1" },
  () => {
    const run = spawnSync(process.execPath, ["--disable-warning=ExperimentalWarning", path.join(here, "..", "readme-media.mjs"), "record"], {
      cwd: repoRoot,
      env: { ...process.env, SCENE: Object.keys(MARKERS).join(",") },
      encoding: "utf8",
      timeout: 20 * 60_000,
    });
    assert.equal(run.status, 0, run.stderr);
    const manifest = readManifest(path.join(repoRoot, ".readme-media", "manifest.json"));
    for (const [name, markers] of Object.entries(MARKERS)) {
      const variants = manifest.scenes[name].variants;
      assert.deepEqual(
        variants.map((v) => v.id),
        ["a", "b"],
      );
      variants.forEach((v, i) => {
        assert.ok(v.duration_s >= 8 && v.duration_s <= 15, `${name}/${v.id}: ${v.duration_s}s`);
        assert.deepEqual(
          v.markers.map((m) => m.name),
          markers[i],
        );
        assert.equal(v.width, 960);
        for (const file of [v.gif, v.poster]) assert.ok(fs.existsSync(path.join(repoRoot, ".readme-media", file)), file);
      });
    }
    // Every demo root is torn down: no daemon, no tmux socket, no auth left.
    const roots = fs.readdirSync(os.tmpdir()).filter((n) => n.startsWith("pdo-readme-media-") && fs.existsSync(path.join(os.tmpdir(), n, "state.json")));
    for (const root of roots) {
      const state = JSON.parse(fs.readFileSync(path.join(os.tmpdir(), root, "state.json"), "utf8"));
      assert.notEqual(state.ownerPid, run.pid, `demo root ${root} of this run survived`);
    }
  },
);
