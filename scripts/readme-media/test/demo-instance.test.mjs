// The demo instance for real: the checkout's daemon on a throwaway cwd, HOME
// and port. Seam 2 of spec #854 (the Stats API of the demo instance) and the
// teardown guarantee (no auth file, tmux session or daemon left behind — after
// a success, a Ctrl+C, a crash, or a hard kill swept by the next start).
//
// Needs the debug binary (`cargo build`); skipped with a message otherwise.

import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { after, before, describe, test } from "node:test";
import { fileURLToPath } from "node:url";
import { DemoInstance, isAlive, sweepStaleInstances } from "../lib/demo-instance.mjs";
import { seedHistory } from "../lib/seed.mjs";
import { parseYaml, readTarget, TARGETS } from "../lib/targets.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..", "..", "..");
const binary = path.join(repoRoot, "target", "debug", "pdo");
const skip = fs.existsSync(binary) ? false : `no ${binary} — run \`cargo build\` first`;

const tmpRoot = fs.mkdtempSync(path.join(os.tmpdir(), "readme-media-it-"));
const SECRET = '{"claudeAiOauth":{"accessToken":"sk-ant-oat-TEST-ONLY"}}';

/** A fake user home with claude auth, so no test ever copies real credentials. */
function fakeHostHome() {
  const home = fs.mkdtempSync(path.join(tmpRoot, "host-"));
  fs.mkdirSync(path.join(home, ".claude"));
  fs.writeFileSync(path.join(home, ".claude", ".credentials.json"), SECRET);
  fs.writeFileSync(
    path.join(home, ".claude.json"),
    JSON.stringify({ oauthAccount: { emailAddress: "demo@example.com" }, userID: "u-1", projects: { "/home/me/secret-project": { mcpServers: {} } } }),
  );
  return home;
}

const tmuxServerUp = (port) => {
  try {
    execFileSync("tmux", ["-L", `pdo-${port}`, "list-sessions"], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
};

function assertGone(state) {
  assert.ok(!isAlive(state.daemonPid), "the demo daemon is stopped");
  assert.ok(!tmuxServerUp(state.port), "no tmux server on the demo socket");
  for (const file of state.authFiles) assert.ok(!fs.existsSync(file), `auth file wiped: ${file}`);
  assert.ok(!fs.existsSync(state.root), "the throwaway root is removed");
}

after(() => fs.rmSync(tmpRoot, { recursive: true, force: true }));

describe("demo instance: Stats API over the mocked history", { skip }, () => {
  let instance;
  let cost;
  let performance;
  const q = "from=1970-01-01T00:00:00Z&to=2100-01-01T00:00:00Z&bucket=day";

  before(async () => {
    instance = new DemoInstance({ repoRoot, tmpRoot, hostHome: fakeHostHome(), liveHarnesses: ["claude"], log: () => {} });
    await instance.start();
    await seedHistory(instance);
    cost = await instance.api("GET", `/stats/cost?${q}`);
    performance = await instance.api("GET", `/stats/performance?${q}&completed_only=false`);
  });
  after(() => instance?.teardown());

  test("sealed: its own cwd (event log), HOME and port; auth staged, and only the account block", () => {
    const { state } = instance;
    assert.ok(state.root.startsWith(tmpRoot));
    assert.notEqual(state.home, os.homedir());
    assert.ok(fs.existsSync(path.join(state.cwd, ".pdo", "pdo.db")), "the event log follows the daemon's cwd");
    assert.notEqual(String(state.port), process.env.PDO_PORT ?? "");
    const staged = path.join(state.home, ".claude", ".credentials.json");
    assert.equal(fs.readFileSync(staged, "utf8"), SECRET);
    assert.equal(fs.statSync(staged).mode & 0o777, 0o600);
    const config = JSON.parse(fs.readFileSync(path.join(state.home, ".claude.json"), "utf8"));
    assert.deepEqual(config.oauthAccount, { emailAddress: "demo@example.com" });
    assert.equal(config.projects["/home/me/secret-project"], undefined, "the user's projects stay home");
    assert.ok(state.authFiles.includes(staged));
  });

  test("each pipeline of the demo library is its target byte for byte, only name: set, and the daemon serves it as drawn", async () => {
    for (const t of TARGETS.filter((target) => target.installed)) {
      const { yaml, prompts } = readTarget(t.id);
      const installed = fs.readFileSync(path.join(instance.libraryDir, `${t.name}.yaml`), "utf8");
      const differing = installed.split("\n").filter((line, i) => line !== yaml.split("\n")[i]);
      assert.ok(differing.every((line) => /^name: /.test(line)), `${t.id}: only name: differs (${JSON.stringify(differing)})`);
      assert.equal(installed.length - installed.indexOf("\n"), yaml.length - yaml.indexOf("\n"), `${t.id}: same bytes after the name line`);
      assert.equal(parseYaml(installed).name, t.name);
      for (const [node, content] of Object.entries(prompts)) {
        assert.equal(fs.readFileSync(path.join(instance.libraryDir, `${t.name}.prompts`, `${node}.md`), "utf8"), content);
      }
      const { pipeline } = await instance.api("GET", `/pipelines/${t.name}`);
      const drawn = parseYaml(yaml);
      assert.deepEqual(
        pipeline.nodes.map((n) => [n.id, n.view?.x, n.view?.y]),
        drawn.nodes.map((n) => [n.id, n.view.x, n.view.y]),
        `${t.id}: the positions as drawn`,
      );
      assert.equal(pipeline.edges.length, drawn.edges.length);
    }
  });

  test("/stats/cost: median cost ratios per model × effort, and no run without computable cost", () => {
    assert.equal(cost.total.unknown, 0, "no « Runs without computable cost » banner");
    assert.deepEqual(cost.total.missing_reasons, []);
    assert.deepEqual(
      cost.by_model.map((m) => m.id).sort(),
      ["claude-fable-5-1", "claude-opus-5-5", "gpt-5.6-sol", "z-ai/glm-5.3-flash"],
    );
    const effort = (model, id) => cost.by_model.find((m) => m.id === model).efforts.find((e) => e.id === id);
    const opus = effort("claude-opus-5-5", "medium").median_usd;
    const ratio = (model, id) => effort(model, id).median_usd / opus;
    assert.ok(ratio("claude-fable-5-1", "low") > 1.03 && ratio("claude-fable-5-1", "low") < 1.35, `fable low ${ratio("claude-fable-5-1", "low")}`);
    assert.ok(ratio("claude-fable-5-1", "high") > 1.8, `fable high ${ratio("claude-fable-5-1", "high")}`);
    assert.ok(ratio("gpt-5.6-sol", "medium") > 0.4 && ratio("gpt-5.6-sol", "medium") < 0.6, `gpt ${ratio("gpt-5.6-sol", "medium")}`);
    const glm = 1 / ratio("z-ai/glm-5.3-flash", "medium");
    assert.ok(glm > 60 && glm < 90, `glm ${glm}`);
    assert.deepEqual(
      cost.by_pipeline.map((p) => p.id),
      ["implement-review"],
      "the Stats are computed on implement-review",
    );
  });

  test("about 50 executions per model × node (± 30 %)", () => {
    for (const model of cost.by_model) {
      const perNode = {};
      for (const effort of model.efforts) {
        for (const pipeline of effort.pipelines) {
          for (const node of pipeline.nodes) perNode[node.name] = (perNode[node.name] ?? 0) + node.executions;
        }
      }
      assert.deepEqual(Object.keys(perNode).sort(), ["implementer", "reviewer"], model.id);
      for (const [node, n] of Object.entries(perNode)) assert.ok(n >= 35 && n <= 65, `${model.id} × ${node}: ${n}`);
    }
  });

  test("/stats/performance against /stats/cost: failure rates (~3 %, GPT a bit more, GLM ~20 %)", () => {
    const rate = (modelId) => {
      const attempts = cost.by_model.find((m) => m.id === modelId).executions;
      const successes = performance.by_model.find((m) => m.id === modelId).harnesses.reduce((sum, h) => sum + h.duration.measured, 0);
      return 1 - successes / attempts;
    };
    for (const model of ["claude-opus-5-5", "claude-fable-5-1"]) assert.ok(rate(model) > 0.01 && rate(model) < 0.05, `${model} ${rate(model)}`);
    assert.ok(rate("gpt-5.6-sol") > rate("claude-opus-5-5") && rate("gpt-5.6-sol") < 0.09, `gpt ${rate("gpt-5.6-sol")}`);
    assert.ok(rate("z-ai/glm-5.3-flash") > 0.15 && rate("z-ai/glm-5.3-flash") < 0.25, `glm ${rate("z-ai/glm-5.3-flash")}`);
  });

  test("Overview, Sessions and Triggers are not empty", async () => {
    const overview = await instance.api("GET", `/stats/overview?${q}`);
    assert.ok(overview.runs.reduce((s, b) => s + b.count, 0) > 150);
    assert.ok(overview.errors.reduce((s, b) => s + b.count, 0) > 0);
    assert.deepEqual(overview.session_harnesses.sort(), ["claude", "copilot", "pi"]);
    assert.equal(overview.fires_by_pipeline[0].pipeline_id, "implement-review");
    assert.equal(overview.fires_by_pipeline[0].count, 100);
  });

  test("teardown leaves no auth file, tmux server or daemon", () => {
    execFileSync("tmux", ["-L", instance.tmuxSocket, "new-session", "-d", "-s", "pdo-demo-agent", "sleep 600"]);
    assert.ok(tmuxServerUp(instance.state.port));
    const state = structuredClone(instance.state);
    instance.teardown();
    assertGone(state);
  });
});

/** Run the helper in a child, wait for its state line. */
function holdInstance(mode = "hold") {
  const child = spawn(process.execPath, ["--disable-warning=ExperimentalWarning", path.join(here, "fixtures", "hold-instance.mjs"), tmpRoot, fakeHostHome(), mode], {
    stdio: ["ignore", "pipe", "pipe"],
  });
  const exited = new Promise((resolve) => child.on("exit", (code, signal) => resolve({ code, signal })));
  const ready = new Promise((resolve, reject) => {
    let out = "";
    child.stdout.on("data", (chunk) => {
      out += chunk;
      const line = out.split("\n").find((l) => l.startsWith("{"));
      if (line) resolve(JSON.parse(line));
    });
    child.on("exit", () => reject(new Error(`helper exited before ready: ${out}`)));
  });
  return { child, ready, exited };
}

describe("demo instance: cleaned up whatever the ending", { skip }, () => {
  test("Ctrl+C (SIGINT) mid-recording", async () => {
    const { child, ready, exited } = holdInstance();
    const state = await ready;
    assert.ok(state.authFiles.length > 0 && isAlive(state.daemonPid) && tmuxServerUp(state.port));
    child.kill("SIGINT");
    assert.equal((await exited).code, 130);
    assertGone(state);
  });

  test("Ctrl+C waits for an agent that outlives the hangup, then removes the root for good", async () => {
    const { child, ready, exited } = holdInstance("stubborn");
    const state = await ready;
    try {
      assert.ok(isAlive(state.agentPid), "the stand-in agent runs");
      child.kill("SIGINT");
      assert.equal((await exited).code, 130);
      // The script handed back only once the agent had really exited…
      assert.ok(!isAlive(state.agentPid), "the agent exited before the script did");
      assertGone(state);
      // …so nothing is left to write under the demo HOME and recreate the root.
      await new Promise((resolve) => setTimeout(resolve, 3000));
      assert.ok(!fs.existsSync(state.root), "the root stays removed");
    } finally {
      // A failing run must not leave the stand-in behind: it would write under
      // the test's tmp root after the suite removed it.
      if (isAlive(state.agentPid)) process.kill(state.agentPid, "SIGKILL");
      fs.rmSync(state.root, { recursive: true, force: true });
    }
  });

  test("an uncaught error in a scene", async () => {
    const { ready, exited } = holdInstance("crash");
    const state = await ready;
    assert.equal((await exited).code, 1);
    assertGone(state);
  });

  test("a hard kill leaves a state file the next start sweeps", async () => {
    const { child, ready, exited } = holdInstance();
    const state = await ready;
    child.kill("SIGKILL");
    await exited;
    assert.ok(isAlive(state.daemonPid), "nothing ran on SIGKILL");
    sweepStaleInstances(tmpRoot, () => {});
    assertGone(state);
  });
});
