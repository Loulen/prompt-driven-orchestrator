// Start and stop of the demo daemon (`make demo-daemon` / `make
// demo-daemon-stop`): the stop only reaches its own pid and its own tmux
// socket, the staging never touches the real harness config. The start of a
// real daemon needs the debug binary (`cargo build`); skipped otherwise.

import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { after, describe, test } from "node:test";
import { fileURLToPath } from "node:url";
import { isAlive } from "../../readme-media/lib/demo-instance.mjs";
import { resyncDemo, stageHarnessCredentials, startDemo, stopDemo } from "../lib/daemon.mjs";
import { demoPaths } from "../lib/env.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");
const tmpRoot = fs.mkdtempSync(path.join(os.tmpdir(), "demo-daemon-it-"));
after(() => fs.rmSync(tmpRoot, { recursive: true, force: true }));
const quiet = () => {};

/** The tmux commands of the test, out of whatever tmux the suite runs in. */
const tmuxEnv = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith("TMUX")));
const tmux = (...args) => execFileSync("tmux", args, { env: tmuxEnv, stdio: ["ignore", "pipe", "ignore"], encoding: "utf8" });
const socketUp = (port) => {
  try {
    tmux("-L", `pdo-${port}`, "list-sessions");
    return true;
  } catch {
    return false;
  }
};

/** A free port in the demo range, none of the user's. */
async function freePort() {
  for (;;) {
    const port = 6400 + Math.floor(Math.random() * 600);
    const free = await new Promise((resolve) => {
      const server = net.createServer();
      server.once("error", () => resolve(false));
      server.listen(port, "127.0.0.1", () => server.close(() => resolve(true)));
    });
    if (free && !socketUp(port)) return port;
  }
}

/** A long-lived process standing for a daemon, in its own group like the real one. */
function sleeper(env = process.env) {
  const child = spawn("sleep", ["300"], { env, detached: true, stdio: "ignore" });
  child.unref();
  return child.pid;
}

function kill(pid) {
  try {
    process.kill(pid, "SIGKILL");
  } catch {
    // gone
  }
}

/** A fake user home with every harness's auth, so no test reads real credentials. */
function fakeHostHome() {
  const home = fs.mkdtempSync(path.join(tmpRoot, "host-"));
  const files = {
    ".claude/.credentials.json": '{"claudeAiOauth":{"accessToken":"TEST-ONLY"}}',
    ".claude.json": JSON.stringify({ oauthAccount: { emailAddress: "demo@example.com" }, projects: { "/home/me/secret": {} } }),
    ".claude/settings.json": '{"model":"mine"}',
    ".pi/agent/auth.json": '{"key":"TEST-ONLY"}',
    ".local/share/opencode/auth.json": '{"key":"TEST-ONLY"}',
    ".copilot/config.json": `// managed automatically\n${JSON.stringify({
      trustedFolders: ["/home/me/secret"],
      lastLoggedInUser: { host: "https://github.com", login: "me" },
      loggedInUsers: [{ host: "https://github.com", login: "me" }],
      recentModelIds: ["gpt-x"],
    })}`,
  };
  for (const [rel, content] of Object.entries(files)) {
    fs.mkdirSync(path.dirname(path.join(home, rel)), { recursive: true });
    fs.writeFileSync(path.join(home, rel), content);
  }
  return home;
}

/** Every file under `dir` with its content and mtime: the proof nothing moved. */
function snapshot(dir) {
  const out = {};
  for (const entry of fs.readdirSync(dir, { recursive: true, withFileTypes: true })) {
    if (!entry.isFile()) continue;
    const file = path.join(entry.parentPath, entry.name);
    out[file] = [fs.readFileSync(file, "utf8"), fs.statSync(file).mtimeMs];
  }
  return out;
}

describe("credentials staging", () => {
  test("copies the harnesses' auth files as private regular files, and leaves the real ones as they were", () => {
    const hostHome = fakeHostHome();
    const before = snapshot(hostHome);
    const demoHome = path.join(tmpRoot, "staged-home");
    const staged = stageHarnessCredentials({ hostHome, demoHome });

    for (const rel of [".claude/.credentials.json", ".pi/agent/auth.json", ".local/share/opencode/auth.json"]) {
      const copy = path.join(demoHome, rel);
      assert.ok(staged.includes(copy), `${rel} recorded`);
      assert.ok(fs.lstatSync(copy).isFile() && !fs.lstatSync(copy).isSymbolicLink(), `${rel} is a copy, not a link`);
      assert.equal(fs.readFileSync(copy, "utf8"), fs.readFileSync(path.join(hostHome, rel), "utf8"));
      assert.equal(fs.statSync(copy).mode & 0o777, 0o600);
    }
    assert.ok(!fs.lstatSync(path.join(demoHome, ".claude")).isSymbolicLink());
    const config = JSON.parse(fs.readFileSync(path.join(demoHome, ".claude.json"), "utf8"));
    assert.equal(config.projects["/home/me/secret"], undefined, "the user's projects stay home");
    assert.notEqual(JSON.parse(fs.readFileSync(path.join(demoHome, ".claude", "settings.json"), "utf8")).model, "mine");
    assert.deepEqual(snapshot(hostHome), before, "the real config is untouched");
  });

  test("copilot gets its logged-in user and trusts /tmp, where the journeys' repos live; not the user's own folders", () => {
    const hostHome = fakeHostHome();
    const demoHome = path.join(tmpRoot, "staged-copilot");
    const staged = stageHarnessCredentials({ hostHome, demoHome });
    const file = path.join(demoHome, ".copilot", "config.json");
    assert.ok(staged.includes(file), "recorded for the wipe");
    const config = JSON.parse(fs.readFileSync(file, "utf8"));
    assert.deepEqual(config.trustedFolders, ["/tmp"]);
    assert.deepEqual(config.loggedInUsers, [{ host: "https://github.com", login: "me" }]);
    assert.deepEqual(config.lastLoggedInUser, { host: "https://github.com", login: "me" });
    assert.equal(config.recentModelIds, undefined);
    assert.equal(fs.statSync(file).mode & 0o777, 0o600);
  });

  test("a harness without auth on this machine is skipped", () => {
    const hostHome = fs.mkdtempSync(path.join(tmpRoot, "bare-host-"));
    const demoHome = path.join(tmpRoot, "bare-staged");
    stageHarnessCredentials({ hostHome, demoHome });
    assert.ok(!fs.existsSync(path.join(demoHome, ".pi")));
    assert.ok(!fs.existsSync(path.join(demoHome, ".copilot")));
    assert.ok(!fs.existsSync(path.join(demoHome, ".claude", ".credentials.json")));
  });
});

// FP of CORE-227: the host refreshed its claude login during a long journey,
// which retires the token the demo HOME held, and the nodes read « Login
// expired ». The resync copies the host's auth again, into the running demo.
describe("resync", () => {
  test("copies the host's current auth into the demo HOME, and lists it for the wipe", () => {
    const hostHome = fakeHostHome();
    const port = 6999;
    const paths = demoPaths(port, tmpRoot);
    fs.mkdirSync(paths.home, { recursive: true });
    fs.writeFileSync(paths.pidfile, "1\n");
    fs.writeFileSync(path.join(paths.root, "credentials.txt"), "/elsewhere/staged-before\n");
    stageHarnessCredentials({ hostHome, demoHome: paths.home });
    const rel = ".claude/.credentials.json";
    fs.writeFileSync(path.join(hostHome, rel), '{"claudeAiOauth":{"accessToken":"REFRESHED"}}');

    // A trust the journey granted, written by claude into the demo's config.
    const config = path.join(paths.home, ".claude.json");
    const granted = { ...JSON.parse(fs.readFileSync(config, "utf8")), projects: { "/tmp/repo": { hasTrustDialogAccepted: true } } };
    fs.writeFileSync(config, JSON.stringify(granted));

    const staged = resyncDemo({ port, tmpRoot, hostHome });

    assert.deepEqual(JSON.parse(fs.readFileSync(config, "utf8")), granted, "the demo's own config is kept");
    assert.equal(fs.readFileSync(path.join(paths.home, rel), "utf8"), '{"claudeAiOauth":{"accessToken":"REFRESHED"}}');
    assert.ok(staged.includes(path.join(paths.home, rel)));
    const listed = fs.readFileSync(path.join(paths.root, "credentials.txt"), "utf8").split("\n").filter(Boolean);
    assert.ok(listed.includes("/elsewhere/staged-before"), "what the start staged stays listed");
    assert.ok(listed.includes(path.join(paths.home, rel)));
    assert.equal(new Set(listed).size, listed.length, "each file listed once");
  });

  test("refuses a port with no demo daemon, and writes nothing", () => {
    const port = 6998;
    assert.throws(() => resyncDemo({ port, tmpRoot, hostHome: fakeHostHome() }), /no demo daemon on port 6998/);
    assert.ok(!fs.existsSync(demoPaths(port, tmpRoot).home));
  });
});

describe("stop", () => {
  test("refuses a port without a pidfile, and stops nothing", async () => {
    const port = await freePort();
    tmux("-L", `pdo-${port}`, "new-session", "-d", "-s", "bystander", "sleep 300");
    try {
      assert.throws(() => stopDemo({ port, tmpRoot, log: quiet }), /no demo daemon on port/);
      assert.ok(socketUp(port), "a tmux server the demo did not start is left alone");
    } finally {
      tmux("-L", `pdo-${port}`, "kill-server");
    }
  });

  test("kills the pid of its pidfile and its own tmux socket, nothing else", async () => {
    const port = await freePort();
    const paths = demoPaths(port, tmpRoot);
    fs.mkdirSync(paths.home, { recursive: true });
    const daemon = sleeper({ ...process.env, HOME: paths.home });
    const bystander = sleeper();
    const otherPort = await freePort();
    tmux("-L", `pdo-${port}`, "new-session", "-d", "-s", "agent", "sleep 300");
    tmux("-L", `pdo-${otherPort}`, "new-session", "-d", "-s", "other", "sleep 300");
    fs.writeFileSync(paths.pidfile, `${daemon}\n`);
    try {
      stopDemo({ port, tmpRoot, log: quiet });
      assert.ok(!isAlive(daemon), "the demo daemon is stopped");
      assert.ok(!socketUp(port), "its tmux server is gone");
      assert.ok(isAlive(bystander), "a process of another HOME is left alone");
      assert.ok(socketUp(otherPort), "another socket's server is left alone");
      assert.ok(!fs.existsSync(paths.root), "the demo folder is removed");
    } finally {
      kill(daemon);
      kill(bystander);
      tmux("-L", `pdo-${otherPort}`, "kill-server");
    }
  });

  test("a pidfile naming a live process that is not the demo daemon's: nothing is stopped", async () => {
    const port = await freePort();
    const paths = demoPaths(port, tmpRoot);
    fs.mkdirSync(paths.home, { recursive: true });
    const stranger = sleeper();
    tmux("-L", `pdo-${port}`, "new-session", "-d", "-s", "other", "sleep 300");
    fs.writeFileSync(paths.pidfile, `${stranger}\n`);
    try {
      assert.throws(() => stopDemo({ port, tmpRoot, log: quiet }), new RegExp(`pid ${stranger} .* nothing stopped`));
      assert.ok(isAlive(stranger), "a recycled pid is never killed");
      assert.ok(socketUp(port), "nor the socket's server");
      assert.ok(fs.existsSync(paths.pidfile));
    } finally {
      kill(stranger);
      tmux("-L", `pdo-${port}`, "kill-server");
    }
  });

  test("a pidfile whose daemon already died: the rest is cleaned up", async () => {
    const port = await freePort();
    const paths = demoPaths(port, tmpRoot);
    fs.mkdirSync(paths.home, { recursive: true });
    const dead = spawn("true");
    await new Promise((resolve) => dead.on("exit", resolve));
    tmux("-L", `pdo-${port}`, "new-session", "-d", "-s", "agent", "sleep 300");
    fs.writeFileSync(paths.pidfile, `${dead.pid}\n`);
    stopDemo({ port, tmpRoot, log: quiet });
    assert.ok(!socketUp(port));
    assert.ok(!fs.existsSync(paths.root));
  });
});

const binary = path.join(repoRoot, "target", "debug", "pdo");
const skip = fs.existsSync(binary) ? false : `no ${binary} — run \`cargo build\` first`;

describe("a real demo daemon", { skip }, () => {
  test("serves on its port from the branch's binary, isolated, then stops for good", async () => {
    const port = await freePort();
    const hostHome = fakeHostHome();
    const caller = { ...process.env, HOME: hostHome, PDO_PORT: "5272", PDO_TOKEN: "pdo_node_x", PDO_NODE_ID: "abc", TMUX: "/tmp/tmux-0/default,1,0" };
    const demo = await startDemo({ port, repoRoot, tmpRoot, env: caller, log: quiet, build: false });
    try {
      assert.equal(demo.url, `http://127.0.0.1:${port}`);
      assert.equal(demo.pdo, binary);
      assert.equal(fs.realpathSync(path.join(demo.paths.bin, "pdo")), fs.realpathSync(binary));
      const runs = execFileSync(binary, ["run", "list"], { env: { ...demo.env, PDO_DAEMON_URL: demo.url }, encoding: "utf8" });
      assert.doesNotMatch(runs, /error/i);
      const environ = fs.readFileSync(`/proc/${demo.pid}/environ`, "utf8").split("\0");
      assert.ok(environ.includes(`HOME=${demo.paths.home}`));
      assert.ok(!environ.some((line) => /^(PDO_TOKEN|PDO_NODE_ID|TMUX)=/.test(line)));
      await assert.rejects(startDemo({ port, repoRoot, tmpRoot, env: caller, log: quiet, build: false }), /already running/);
    } finally {
      stopDemo({ port, tmpRoot, log: quiet });
    }
    assert.ok(!isAlive(demo.pid));
    assert.ok(!fs.existsSync(demo.paths.root));
  });
});
