// The media config (#36, lib/config.mjs): the core's own first, then another
// repo's, written in a throwaway folder outside the core. Every error names the
// field or the path; a third-party publication lands in ITS destination, never
// in the core's.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { after, describe, test } from "node:test";
import { fileURLToPath } from "node:url";
import { assertRecordable, CORE_CONFIG, ConfigError, demoOptions, loadConfig, resolveConfig, resolvePlaywright, TOOL_DIR } from "../lib/config.mjs";
import { readManifest } from "../lib/manifest.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..", "..", "..");
const script = path.join(here, "..", "readme-media.mjs");
const coreBinary = path.join(repoRoot, "target", "debug", "pdo");

const tmpRoot = fs.mkdtempSync(path.join(os.tmpdir(), "readme-media-config-"));
after(() => fs.rmSync(tmpRoot, { recursive: true, force: true }));

const variant = (id) => `{ id: "${id}", viewport: { width: 800, height: 600 }, markers: ["m"], async play() {} }`;

/** Another repo: a scene, a selection, a config of its own — relative paths. */
function consumerRepo({ config = null, binary = "./bin/pdo" } = {}) {
  const dir = fs.mkdtempSync(path.join(tmpRoot, "consumer-"));
  fs.mkdirSync(path.join(dir, "media", "scenes"), { recursive: true });
  fs.writeFileSync(path.join(dir, "media", "scenes", "board.mjs"), `export default { name: "board", variants: [${variant("a")}, ${variant("b")}] };\n`);
  fs.writeFileSync(path.join(dir, "media", "selection.txt"), "board  b\n");
  fs.mkdirSync(path.join(dir, "media", "bin"));
  const file = path.join(dir, "media", "media.config.mjs");
  fs.writeFileSync(
    file,
    config ??
      `export default {
  binary: ${JSON.stringify(binary)},
  scenes: "./scenes",
  selection: "./selection.txt",
  review: "../.readme-media",
  destination: "../docs/media",
};
`,
  );
  return { dir, file };
}

/** A stand-in binary: an executable file (record never gets past the browser here). */
function fakeBinary(file, mode = 0o755) {
  fs.writeFileSync(file, "#!/bin/sh\nexit 0\n");
  fs.chmodSync(file, mode);
}

const BASE = { binary: "./bin/pdo", scenes: "./scenes", selection: "./selection.txt", review: "../.readme-media", destination: "../docs/media" };

const run = (args, opts = {}) =>
  spawnSync(process.execPath, ["--disable-warning=ExperimentalWarning", script, ...args], { cwd: repoRoot, encoding: "utf8", timeout: 60_000, ...opts });

describe("the core's config", () => {
  test("is the tool's own file, and names today's paths: its scenes, selection, review folder and README assets", async () => {
    assert.equal(CORE_CONFIG, path.join(TOOL_DIR, "readme-media.config.mjs"));
    const core = await loadConfig();
    assert.equal(core.binary, coreBinary);
    assert.equal(core.scenes, path.join(TOOL_DIR, "scenes"));
    assert.equal(core.selection, path.join(TOOL_DIR, "selection.txt"));
    assert.equal(core.review, path.join(repoRoot, ".readme-media"));
    assert.equal(core.destination, path.join(repoRoot, "docs", "assets", "readme"));
    assert.equal(core.fixture.repo, path.join(TOOL_DIR, "fixture", "shop-app"));
    assert.equal(typeof core.fixture.pipelines, "function");
    assert.equal(typeof core.seed, "function");
    assert.equal(core.playwright, path.join(repoRoot, "frontend"));
    assert.deepEqual(demoOptions(core), { binary: coreBinary, fixtureRepo: core.fixture.repo, pipelines: core.fixture.pipelines });
  });

  test("its bootstrap seeds the mocked history only for a scene that needs it", async () => {
    const core = await loadConfig();
    const calls = [];
    const instance = { api: async (...args) => calls.push(args) };
    await core.seed(instance, { name: "pipelines", needs: [] });
    await core.seed(instance, { name: "hero" });
    assert.deepEqual(calls, [], "no API call without `needs: [\"history\"]`");
  });
});

describe("another repo's config", () => {
  test("its relative paths resolve from its own folder, never the core's or the cwd", async () => {
    const { dir, file } = consumerRepo();
    const config = await loadConfig(file);
    assert.equal(config.file, file);
    assert.equal(config.binary, path.join(dir, "media", "bin", "pdo"));
    assert.equal(config.scenes, path.join(dir, "media", "scenes"));
    assert.equal(config.selection, path.join(dir, "media", "selection.txt"));
    assert.equal(config.review, path.join(dir, ".readme-media"));
    assert.equal(config.destination, path.join(dir, "docs", "media"));
    assert.deepEqual(config.fixture, { repo: null, pipelines: null }, "no fixture: no repo, an empty library");
    assert.equal(config.seed, null, "no bootstrap: no mocked history");
    assert.equal(config.playwright, path.join(repoRoot, "frontend"), "Playwright from the core checkout by default");
  });

  test("a fixture folder, a library folder, a seed and a Playwright folder of its own", () => {
    const { dir } = consumerRepo();
    for (const sub of ["fixture/repo", "fixture/pipelines", "node"]) fs.mkdirSync(path.join(dir, "media", sub), { recursive: true });
    const seed = async () => {};
    const config = resolveConfig(
      { ...BASE, fixture: { repo: "./fixture/repo", pipelines: "./fixture/pipelines" }, seed, playwright: "./node" },
      path.join(dir, "media", "media.config.mjs"),
    );
    assert.deepEqual(config.fixture, { repo: path.join(dir, "media", "fixture", "repo"), pipelines: path.join(dir, "media", "fixture", "pipelines") });
    assert.equal(config.seed, seed);
    assert.equal(config.playwright, path.join(dir, "media", "node"));
  });
});

describe("an incomplete or wrong config names the field or the path", () => {
  const { dir } = consumerRepo();
  const file = path.join(dir, "media", "media.config.mjs");
  const refused = (raw, pattern) => assert.throws(() => resolveConfig(raw, file), (error) => error instanceof ConfigError && pattern.test(error.message) && error.message.includes(file));

  for (const field of ["binary", "scenes", "selection", "review", "destination"]) {
    test(`missing \`${field}\``, () => {
      const { [field]: _, ...rest } = BASE;
      refused(rest, new RegExp(`missing \`${field}\``));
    });
  }

  test("a misspelt field", () => refused({ ...BASE, destinaton: "x" }, /unknown field\(s\) `destinaton`/));
  test("a misspelt fixture field", () => refused({ ...BASE, fixture: { repos: "x" } }, /`fixture\.repos`/));
  test("a path that is not a string", () => refused({ ...BASE, review: 3 }, /`review` must be a path/));
  test("a seed that is not a function", () => refused({ ...BASE, seed: "seed.sh" }, /`seed` must be a function/));
  test("pipelines neither a folder nor a function", () => refused({ ...BASE, fixture: { pipelines: 1 } }, /`fixture\.pipelines` must be/));
  test("no default export", () => refused(undefined, /must `export default` a config object/));

  for (const [key, raw, missing] of [
    ["scenes", { ...BASE, scenes: "./nope" }, path.join(dir, "media", "nope")],
    ["selection", { ...BASE, selection: "./nope.txt" }, path.join(dir, "media", "nope.txt")],
    ["fixture.repo", { ...BASE, fixture: { repo: "./nope" } }, path.join(dir, "media", "nope")],
    ["fixture.pipelines", { ...BASE, fixture: { pipelines: "./nope" } }, path.join(dir, "media", "nope")],
    ["playwright", { ...BASE, playwright: "./nope" }, path.join(dir, "media", "nope")],
  ]) {
    test(`\`${key}\` that does not exist: the field and the resolved path`, () => {
      assert.throws(
        () => resolveConfig(raw, file),
        (error) => error.message.includes(`\`${key}\` → ${missing}`),
      );
    });
  }

  test("scenes that are a file, not a folder", () => refused({ ...BASE, scenes: "./selection.txt" }, /`scenes` → .*selection\.txt: not a folder/));

  test("record: an absent binary, or one that does not run", () => {
    const config = resolveConfig(BASE, file);
    assert.throws(() => assertRecordable(config), (error) => error.message.includes(`\`binary\` → ${path.join(dir, "media", "bin", "pdo")}: no such file`));
    fakeBinary(path.join(dir, "media", "bin", "pdo"), 0o644);
    assert.throws(() => assertRecordable(config), /`binary` → .*: not executable/);
    fs.chmodSync(path.join(dir, "media", "bin", "pdo"), 0o755);
    assertRecordable(config);
  });

  test("a Playwright folder @playwright/test does not resolve from", () => {
    const config = resolveConfig({ ...BASE, playwright: "./scenes" }, file);
    assert.throws(() => resolvePlaywright(config), /`playwright` → .*scenes: @playwright\/test does not resolve from there/);
  });

  test("a config file that does not exist", async () => {
    await assert.rejects(loadConfig(path.join(dir, "missing.config.mjs")), /readme-media config not found: .*missing\.config\.mjs/);
  });
});

describe("the command line with another repo's config", () => {
  test("record with an incomplete config fails before any recording, naming the field", () => {
    const { file } = consumerRepo({ config: 'export default { binary: "./bin/pdo", scenes: "./scenes", selection: "./selection.txt", review: "../.readme-media" };\n' });
    const out = run(["record", "--config", file]);
    assert.equal(out.status, 1);
    assert.match(out.stderr, /missing `destination`/);
    assert.ok(out.stderr.includes(file));
  });

  test("record with an absent binary fails naming its path, and records nothing", () => {
    const { dir, file } = consumerRepo({ binary: "./bin/acme-pdo" });
    const out = run(["record", `--config=${file}`]);
    assert.equal(out.status, 1);
    assert.ok(out.stderr.includes(`\`binary\` → ${path.join(dir, "media", "bin", "acme-pdo")}: no such file`), out.stderr);
    assert.ok(!fs.existsSync(path.join(dir, ".readme-media")), "no review folder");
  });

  test("publish copies the selected variant into ITS destination, and leaves the core's assets alone", () => {
    const { dir, file } = consumerRepo();
    for (const v of ["a", "b"]) {
      fs.mkdirSync(path.join(dir, ".readme-media", "board"), { recursive: true });
      fs.writeFileSync(path.join(dir, ".readme-media", "board", `${v}.gif`), `gif ${v}`);
      fs.writeFileSync(path.join(dir, ".readme-media", "board", `${v}.jpg`), `jpg ${v}`);
    }
    const coreAssets = path.join(repoRoot, "docs", "assets", "readme");
    const before = fs.readdirSync(coreAssets).map((name) => [name, fs.statSync(path.join(coreAssets, name)).mtimeMs]);
    // From another cwd: the paths follow the config, not the caller.
    const out = run(["publish", "--config", file], { cwd: tmpRoot });
    assert.equal(out.status, 0, out.stderr);
    assert.match(out.stdout, /published board ← variant b/);
    assert.equal(fs.readFileSync(path.join(dir, "docs", "media", "board.gif"), "utf8"), "gif b");
    assert.equal(fs.readFileSync(path.join(dir, "docs", "media", "board.jpg"), "utf8"), "jpg b");
    assert.deepEqual(
      fs.readdirSync(coreAssets).map((name) => [name, fs.statSync(path.join(coreAssets, name)).mtimeMs]),
      before,
      "the core's published media are untouched",
    );
    assert.ok(!fs.existsSync(path.join(coreAssets, "board.gif")));
  });

  test("export and import are the core's own: refused with another repo's config", () => {
    const { file } = consumerRepo();
    for (const command of ["export", "import"]) {
      const out = run([command, "--config", file]);
      assert.equal(out.status, 1);
      assert.match(out.stderr, new RegExp(`\`${command}\` moves the core's target pipelines only`));
    }
  });

  test("make readme-media-export / -import pass CONFIG on, so they refuse another repo's config too", () => {
    const { file } = consumerRepo();
    for (const target of ["readme-media-export", "readme-media-import"]) {
      const out = spawnSync("make", ["--no-print-directory", "-C", repoRoot, target, `CONFIG=${file}`], { encoding: "utf8", timeout: 60_000 });
      assert.notEqual(out.status, 0, `${target} with CONFIG must fail:\n${out.stdout}`);
      assert.match(out.stderr, /moves the core's target pipelines only/);
    }
  });

  test("an unknown option is refused", () => {
    const out = run(["publish", "--cfg", "x"]);
    assert.equal(out.status, 1);
    assert.match(out.stderr, /unknown option "--cfg"/);
  });
});

// The whole seam for another repo, opt-in (it films for about a minute): a
// folder outside the core, another binary path, a scene with no mocked history
// and no live harness, a bootstrap through the demo's CLI → two variants in ITS
// review folder, the selected one published in ITS destination.
test(
  "record + publish from another repo: a minimal scene, its own review folder and destination",
  { skip: process.env.READMEMEDIA_E2E === "1" ? (fs.existsSync(coreBinary) ? false : `no ${coreBinary}`) : "set READMEMEDIA_E2E=1" },
  () => {
    const { dir, file } = consumerRepo({
      binary: "./bin/pdo-other",
      config: `import fs from "node:fs";
export default {
  binary: "./bin/pdo-other",
  scenes: "./scenes",
  selection: "./selection.txt",
  review: "../.readme-media",
  destination: "../docs/media",
  async seed(instance, scene) {
    // A consumer's bootstrap: its CLI on the demo instance, no mocked history.
    const out = await instance.exec("pdo", ["ext"]);
    fs.writeFileSync(new URL("./seeded.txt", import.meta.url), \`\${scene.name}: \${instance.url}\\n\${out}\`);
  },
};
`,
    });
    fs.symlinkSync(coreBinary, path.join(dir, "media", "bin", "pdo-other"));
    const scene = (id) => `{
      id: "${id}",
      label: "open Stats",
      viewport: { width: 1100, height: 700 },
      markers: ["opened"],
      async play(ctx) {
        await ctx.goto("/");
        await ctx.page.getByTestId("open-stats").waitFor();
        await ctx.hold(800);
        await ctx.keep(() => ctx.click(ctx.page.getByTestId("open-stats"), { duration: 700 }));
        await ctx.page.getByTestId("stats-modal").waitFor();
        ctx.mark("opened");
        await ctx.hold(1500);
      },
    }`;
    fs.writeFileSync(path.join(dir, "media", "scenes", "board.mjs"), `export default { name: "board", title: "Board", live: [], variants: [${scene("a")}, ${scene("b")}] };\n`);
    const demoRoots = () => new Set(fs.readdirSync(os.tmpdir()).filter((name) => name.startsWith("pdo-readme-media-")));
    const rootsBefore = demoRoots();
    const coreReview = path.join(repoRoot, ".readme-media", "manifest.json");
    const coreManifestBefore = fs.existsSync(coreReview) ? fs.readFileSync(coreReview, "utf8") : null;

    const recorded = run(["record", "--config", file], { cwd: tmpRoot, timeout: 10 * 60_000 });
    assert.equal(recorded.status, 0, recorded.stderr);
    const manifest = readManifest(path.join(dir, ".readme-media", "manifest.json"));
    assert.deepEqual(
      manifest.scenes.board.variants.map((v) => v.id),
      ["a", "b"],
    );
    for (const v of manifest.scenes.board.variants) {
      for (const f of [v.gif, v.poster]) assert.ok(fs.existsSync(path.join(dir, ".readme-media", f)), f);
    }
    assert.match(fs.readFileSync(path.join(dir, "media", "seeded.txt"), "utf8"), /^board: http:\/\/127\.0\.0\.1:\d+\n/);

    const published = run(["publish", "--config", file], { cwd: tmpRoot });
    assert.equal(published.status, 0, published.stderr);
    assert.ok(fs.statSync(path.join(dir, "docs", "media", "board.gif")).size > 0);
    assert.ok(fs.statSync(path.join(dir, "docs", "media", "board.jpg")).size > 0);
    assert.equal(fs.existsSync(coreReview) ? fs.readFileSync(coreReview, "utf8") : null, coreManifestBefore, "the core's review folder is untouched");
    assert.deepEqual([...demoRoots()].filter((name) => !rootsBefore.has(name)), [], "no demo instance left");
  },
);
