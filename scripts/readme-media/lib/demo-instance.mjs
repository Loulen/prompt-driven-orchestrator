// The demo instance (ADR-0074 §4, CONTEXT.md « Instance de démo »): a PDO
// daemon sealed off from the user's own instance BY CONSTRUCTION —
//
//   - its own working directory (the event log `.pdo/pdo.db` follows the cwd);
//   - its own HOME (pipelines, skill bank, profiles, prices, transcripts);
//   - its own port (the tmux socket `pdo-<port>` follows the port);
//   - the checkout's binary, and a `pdo` first on the node sessions' PATH that
//     points at it.
//
// Nothing here reads or writes the user's `~/.pdo`, `~/.claude` (except the
// auth files it COPIES for a live harness), daemon or tmux sessions.
//
// Teardown is synchronous so the exit guard can run it from `process.on("exit")`:
// it stops the daemon, kills the demo tmux server (every agent session with it),
// wipes the staged auth files and removes the throwaway root. A run killed hard
// (SIGKILL, power loss) leaves a `state.json` behind; the next start sweeps it.

import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { stageCredentials, wipeCredentials } from "./credentials.mjs";

const ROOT_PREFIX = "pdo-readme-media-";
/** The user's usual ports, never taken even when free. */
const RESERVED_PORTS = new Set([5172, 5173, 5174, 6160, 6172]);

export class DemoInstance {
  /** `hostHome` is where live-harness auth is copied FROM (the user's home;
   *  tests pass a fake one). `tmpRoot` holds the throwaway roots. */
  constructor({ repoRoot, liveHarnesses = [], keep = false, log = console.log, tmpRoot = os.tmpdir(), hostHome = os.homedir() }) {
    this.repoRoot = repoRoot;
    this.liveHarnesses = liveHarnesses;
    this.keep = keep;
    this.log = log;
    this.tmpRoot = tmpRoot;
    this.hostHome = hostHome;
    this.state = null;
  }

  get url() {
    return `http://127.0.0.1:${this.state.port}`;
  }
  get home() {
    return this.state.home;
  }
  get cwd() {
    return this.state.cwd;
  }
  get dbPath() {
    return path.join(this.state.cwd, ".pdo", "pdo.db");
  }
  get repo() {
    return this.state.repo;
  }
  get tmuxSocket() {
    return `pdo-${this.state.port}`;
  }

  async start() {
    sweepStaleInstances(this.tmpRoot, this.log);
    const binary = path.join(this.repoRoot, "target", "debug", "pdo");
    if (!fs.existsSync(binary)) throw new Error(`no PDO binary at ${binary}: run \`make build\` first`);

    const root = fs.mkdtempSync(path.join(this.tmpRoot, ROOT_PREFIX));
    const port = await freePort();
    this.state = {
      ownerPid: process.pid,
      root,
      home: path.join(root, "home"),
      cwd: path.join(root, "daemon"),
      bin: path.join(root, "bin"),
      repo: path.join(root, "repos", "shop-app"),
      port,
      daemonPid: null,
      authFiles: [],
      keep: this.keep,
    };
    this.saveState();
    registerForExit(this);

    for (const dir of [this.state.home, this.state.cwd, this.state.bin]) fs.mkdirSync(dir, { recursive: true });
    fs.symlinkSync(binary, path.join(this.state.bin, "pdo"));
    // The demo tmux server reads the demo HOME's conf: with focus events on,
    // Claude Code never prints its « tmux focus-events off » hint on camera.
    fs.writeFileSync(path.join(this.state.home, ".tmux.conf"), "set -g focus-events on\n");
    this.installFixtureRepo();
    this.installLibrary();

    if (this.liveHarnesses.length > 0) {
      stageCredentials({
        harnesses: this.liveHarnesses,
        hostHome: this.hostHome,
        demoHome: this.state.home,
        trustedDirs: [this.state.repo],
        onStaged: (file) => {
          this.state.authFiles.push(file);
          this.saveState();
        },
      });
    }

    const logFile = fs.openSync(path.join(root, "daemon.log"), "a");
    const child = spawn(binary, ["daemon"], {
      cwd: this.state.cwd,
      env: this.daemonEnv(),
      detached: true,
      stdio: ["ignore", logFile, logFile],
    });
    fs.closeSync(logFile);
    this.state.daemonPid = child.pid;
    this.saveState();
    child.unref();
    await this.waitReady();
    this.log(`demo instance up: ${this.url} (root ${root})`);
    return this;
  }

  /** The daemon's environment: the user's, minus every PDO_* (this process may
   *  itself run inside a PDO node), with the demo HOME, port and PATH. */
  daemonEnv() {
    const env = {};
    for (const [key, value] of Object.entries(process.env)) {
      if (!key.startsWith("PDO_")) env[key] = value;
    }
    const userPath = `${this.state.bin}:${interactivePath()}`;
    Object.assign(env, {
      HOME: this.state.home,
      PDO_PORT: String(this.state.port),
      PDO_BIND: "127.0.0.1",
      PATH: `${this.state.bin}:${process.env.PATH ?? ""}`,
      // The node sessions' PATH (ADR-0055 resolves it from the interactive
      // shell, which under the demo HOME would lose the user's managers): the
      // demo `pdo` first, then the user's own tools, harnesses included.
      PDO_HARNESS_PROBE_PATH: userPath,
      // No egress that could put a banner on screen or move the prices.
      PDO_UPDATE_CHECK: "off",
      PDO_PRICE_SYNC: "off",
      // A live reviewer drives Playwright: its browsers stay in the user's
      // cache (read-only use), not a download into the throwaway HOME.
      PLAYWRIGHT_BROWSERS_PATH: process.env.PLAYWRIGHT_BROWSERS_PATH ?? path.join(os.homedir(), ".cache", "ms-playwright"),
    });
    delete env.TMUX;
    delete env.TMUX_PANE;
    return env;
  }

  installFixtureRepo() {
    const source = path.join(this.repoRoot, "scripts", "readme-media", "fixture", "shop-app");
    fs.mkdirSync(path.dirname(this.state.repo), { recursive: true });
    fs.cpSync(source, this.state.repo, { recursive: true });
    fs.writeFileSync(path.join(this.state.repo, ".gitignore"), ".pdo/\n");
    const git = (...args) => execFileSync("git", args, { cwd: this.state.repo, stdio: "ignore", env: { ...process.env, HOME: this.state.home } });
    git("init", "-q", "-b", "main");
    git("config", "user.email", "demo@example.com");
    git("config", "user.name", "Demo");
    git("config", "commit.gpgsign", "false");
    git("add", ".");
    git("commit", "-q", "-m", "shop-app: initial static shop");
  }

  installLibrary() {
    const source = path.join(this.repoRoot, "scripts", "readme-media", "fixture", "pipelines");
    fs.cpSync(source, path.join(this.state.home, ".pdo", "pipelines"), { recursive: true });
  }

  async waitReady(timeoutMs = 60_000) {
    const deadline = Date.now() + timeoutMs;
    let lastError = null;
    while (Date.now() < deadline) {
      if (!isAlive(this.state.daemonPid)) {
        throw new Error(`demo daemon exited during startup — see ${path.join(this.state.root, "daemon.log")}`);
      }
      try {
        const response = await fetch(`${this.url}/pipelines`);
        if (response.ok) return;
      } catch (error) {
        lastError = error;
      }
      await sleep(250);
    }
    throw new Error(`demo daemon not ready after ${timeoutMs} ms: ${lastError?.message ?? "no answer"}`);
  }

  async api(method, route, body) {
    const response = await fetch(`${this.url}${route}`, {
      method,
      headers: body === undefined ? {} : { "content-type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const text = await response.text();
    if (!response.ok) throw new Error(`${method} ${route} → ${response.status}: ${text}`);
    return text ? JSON.parse(text) : null;
  }

  saveState() {
    fs.writeFileSync(path.join(this.state.root, "state.json"), `${JSON.stringify(this.state, null, 2)}\n`);
  }

  /** Stop everything the instance started. Synchronous and idempotent. */
  teardown() {
    if (!this.state || this.state.tornDown) return;
    teardownState(this.state, this.log);
    this.state.tornDown = true;
    unregisterForExit(this);
  }
}

/** How long an agent gets to exit on its own after the tmux server is gone,
 *  before it is killed. Claude Code takes a few seconds to wind down. */
const AGENT_EXIT_GRACE_MS = 10_000;

/** Synchronous teardown of one instance, from its state (live or found on disk). */
export function teardownState(state, log = console.log) {
  if (state.daemonPid && isAlive(state.daemonPid)) {
    signalGroup(state.daemonPid, "SIGTERM");
    const deadline = Date.now() + 8_000;
    while (isAlive(state.daemonPid) && Date.now() < deadline) sleepSync(100);
    if (isAlive(state.daemonPid)) signalGroup(state.daemonPid, "SIGKILL");
  }
  // The agents are listed BEFORE the tmux server goes: once it is killed its
  // panes are gone, but a `claude` that got the hangup keeps running a few
  // seconds, and on its way out writes under its HOME — which recreated the
  // throwaway root after it was removed (#859). So the root is removed only
  // once every process of the demo has really exited.
  const agents = demoProcesses(state);
  // Every agent the demo launched lives on this socket: killing the server
  // ends them all, and touches no other socket (the user's included).
  try {
    execFileSync("tmux", ["-L", `pdo-${state.port}`, "kill-server"], { stdio: "ignore" });
  } catch {
    // no server: nothing was launched, or it is already gone
  }
  stopProcesses(new Set([...agents, ...demoProcesses(state)]), log);
  // A server that exited on its own (its last session stopped) can leave its
  // socket file behind; the server is gone now, so the file is only litter.
  if (typeof process.getuid === "function") {
    fs.rmSync(path.join(process.env.TMUX_TMPDIR || "/tmp", `tmux-${process.getuid()}`, `pdo-${state.port}`), { force: true });
  }
  const left = wipeCredentials(state.authFiles ?? []);
  if (left.length > 0) log(`WARNING: could not wipe demo auth files: ${left.join(", ")}`);
  if (state.keep) {
    log(`demo instance kept for inspection (auth wiped): ${state.root}`);
    try {
      fs.rmSync(path.join(state.root, "state.json"), { force: true });
    } catch {
      // best effort
    }
  } else {
    // A process started in between (the daemon's last spawn) is stopped too,
    // and the root removed again if it wrote there meanwhile.
    for (let pass = 0; pass < 3; pass++) {
      fs.rmSync(state.root, { recursive: true, force: true });
      const late = demoProcesses(state);
      if (late.size === 0 && !fs.existsSync(state.root)) break;
      stopProcesses(late, log);
    }
    if (fs.existsSync(state.root)) log(`WARNING: could not remove the demo root ${state.root}`);
  }
}

/**
 * Every live process of the demo instance: the panes of its tmux socket and
 * their descendants, plus — where /proc exists — any process whose HOME is the
 * demo HOME or whose cwd is under the root (an agent's child that outlived its
 * parent and was reparented). Never this process.
 */
export function demoProcesses(state) {
  const pids = new Set();
  let panes = [];
  try {
    panes = execFileSync("tmux", ["-L", `pdo-${state.port}`, "list-panes", "-a", "-F", "#{pane_pid}"], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    })
      .split("\n")
      .map(Number)
      .filter(Boolean);
  } catch {
    // no server
  }
  const children = processTree();
  const stack = [...panes];
  while (stack.length > 0) {
    const pid = stack.pop();
    if (pids.has(pid)) continue;
    pids.add(pid);
    stack.push(...(children.get(pid) ?? []));
  }
  if (state.home && state.root) {
    for (const pid of procPidsUsing(state)) pids.add(pid);
  }
  pids.delete(process.pid);
  for (const pid of pids) if (!isAlive(pid)) pids.delete(pid);
  return pids;
}

/** pid → child pids, from `ps` (Linux and macOS). */
function processTree() {
  const children = new Map();
  let out = "";
  try {
    out = execFileSync("ps", ["-A", "-o", "pid=,ppid="], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] });
  } catch {
    return children;
  }
  for (const line of out.split("\n")) {
    const [pid, ppid] = line.trim().split(/\s+/).map(Number);
    if (!pid) continue;
    if (!children.has(ppid)) children.set(ppid, []);
    children.get(ppid).push(pid);
  }
  return children;
}

/** Linux only: processes whose environment names the demo HOME, or whose cwd
 *  is under the demo root. Other users' processes are unreadable and skipped. */
function procPidsUsing(state) {
  const pids = [];
  let entries = [];
  try {
    entries = fs.readdirSync("/proc").filter((name) => /^\d+$/.test(name));
  } catch {
    return pids;
  }
  const home = `HOME=${state.home}`;
  for (const name of entries) {
    const pid = Number(name);
    if (pid === process.pid) continue;
    try {
      const cwd = fs.readlinkSync(`/proc/${name}/cwd`);
      if (cwd === state.root || cwd.startsWith(`${state.root}/`)) {
        pids.push(pid);
        continue;
      }
    } catch {
      // gone, or not ours
    }
    try {
      if (fs.readFileSync(`/proc/${name}/environ`, "utf8").split("\0").includes(home)) pids.push(pid);
    } catch {
      // gone, or not ours
    }
  }
  return pids;
}

/** SIGTERM, wait for the real exit (up to the grace), then SIGKILL. */
function stopProcesses(pids, log) {
  const alive = () => [...pids].filter(isAlive);
  if (alive().length === 0) return;
  for (const pid of alive()) signalPid(pid, "SIGTERM");
  const deadline = Date.now() + AGENT_EXIT_GRACE_MS;
  while (alive().length > 0 && Date.now() < deadline) sleepSync(100);
  const stubborn = alive();
  if (stubborn.length === 0) return;
  log(`demo teardown: killing ${stubborn.length} process(es) still running after ${AGENT_EXIT_GRACE_MS / 1000}s: ${stubborn.join(", ")}`);
  for (const pid of stubborn) signalPid(pid, "SIGKILL");
  const killDeadline = Date.now() + 3_000;
  while (alive().length > 0 && Date.now() < killDeadline) sleepSync(50);
}

function signalPid(pid, signal) {
  try {
    process.kill(pid, signal);
  } catch {
    // already gone
  }
}

/** A previous run killed hard left its state behind: finish its teardown. */
export function sweepStaleInstances(tmpRoot = os.tmpdir(), log = console.log) {
  let entries = [];
  try {
    entries = fs.readdirSync(tmpRoot).filter((name) => name.startsWith(ROOT_PREFIX));
  } catch {
    return;
  }
  for (const name of entries) {
    const stateFile = path.join(tmpRoot, name, "state.json");
    let state;
    try {
      state = JSON.parse(fs.readFileSync(stateFile, "utf8"));
    } catch {
      continue;
    }
    if (state.ownerPid !== process.pid && !isAlive(state.ownerPid)) {
      log(`cleaning up a stale demo instance: ${state.root}`);
      teardownState(state, log);
    }
  }
}

// ---- the exit guard -------------------------------------------------------

const live = new Set();
let guardInstalled = false;

function registerForExit(instance) {
  live.add(instance);
  if (guardInstalled) return;
  guardInstalled = true;
  const teardownAll = () => {
    for (const instance of [...live]) {
      try {
        instance.teardown();
      } catch (error) {
        console.error(`demo teardown failed: ${error?.stack ?? error}`);
      }
    }
  };
  process.on("exit", teardownAll);
  for (const [signal, code] of [
    ["SIGINT", 130],
    ["SIGTERM", 143],
    ["SIGHUP", 129],
  ]) {
    process.on(signal, () => {
      console.error(`\n${signal}: stopping the demo instance…`);
      teardownAll();
      process.exit(code);
    });
  }
  process.on("uncaughtException", (error) => {
    console.error(error?.stack ?? error);
    teardownAll();
    process.exit(1);
  });
}

function unregisterForExit(instance) {
  live.delete(instance);
}

// ---- helpers ---------------------------------------------------------------

/** Alive and not a zombie: the daemon is our child, and a synchronous
 *  teardown never lets the event loop reap it, so a dead daemon lingers as a
 *  zombie that `kill(pid, 0)` still reaches. */
export function isAlive(pid) {
  if (!pid) return false;
  try {
    process.kill(pid, 0);
  } catch (error) {
    if (error.code !== "EPERM") return false;
  }
  try {
    const stat = fs.readFileSync(`/proc/${pid}/stat`, "utf8");
    return stat.slice(stat.lastIndexOf(")") + 2)[0] !== "Z";
  } catch {
    // no procfs (macOS): ask ps
  }
  try {
    return !execFileSync("ps", ["-o", "stat=", "-p", String(pid)], { encoding: "utf8" }).trim().startsWith("Z");
  } catch {
    return false;
  }
}

function signalGroup(pid, signal) {
  try {
    process.kill(-pid, signal);
  } catch {
    try {
      process.kill(pid, signal);
    } catch {
      // already gone
    }
  }
}

function sleepSync(ms) {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

export function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function freePort() {
  for (let attempt = 0; attempt < 20; attempt++) {
    const port = await new Promise((resolve, reject) => {
      const server = net.createServer();
      server.once("error", reject);
      server.listen(0, "127.0.0.1", () => {
        const { port } = server.address();
        server.close(() => resolve(port));
      });
    });
    if (!RESERVED_PORTS.has(port) && String(port) !== process.env.PDO_PORT) return port;
  }
  throw new Error("no free port for the demo daemon");
}

/** The user's interactive-shell PATH (where nvm, Homebrew & co. put the
 *  harnesses), read with the REAL home — the demo HOME has no rc files. */
function interactivePath() {
  const shell = process.env.SHELL;
  if (shell) {
    try {
      const out = execFileSync(shell, ["-i", "-c", 'printf %s "$PATH"'], {
        encoding: "utf8",
        stdio: ["ignore", "pipe", "ignore"],
        timeout: 15_000,
      });
      if (out.trim()) return out.trim();
    } catch {
      // fall back to this process's PATH
    }
  }
  return process.env.PATH ?? "";
}
