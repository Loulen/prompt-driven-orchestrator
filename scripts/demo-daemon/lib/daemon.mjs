// Start and stop a demo daemon (`make demo-daemon` / `make demo-daemon-stop`):
// the branch's `pdo daemon`, or the demo daemon of the test extension
// (`crates/pdo/tests/demo_daemon.rs`), on a port the caller names, under a
// fresh HOME in /tmp/pdo-demo-<port> (lib/env.mjs).
//
// The start writes a pidfile; the stop reaches nothing the pidfile does not
// name: the pid it holds, the tmux server of the socket `pdo-<port>`, and the
// processes still running under the demo HOME. A pidfile whose live pid does
// not run under the demo HOME stops nothing. Never the default tmux server,
// never the user's daemon.

import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
import path from "node:path";
import { HARNESS_AUTH, stageCredentials } from "../../readme-media/lib/credentials.mjs";
import { isAlive, sleep, teardownState } from "../../readme-media/lib/demo-instance.mjs";
import { demoEnv, demoPaths, TMUX_VARS } from "./env.mjs";

/** The files staged into the demo HOME, listed for the stop's wipe. */
const STAGED_LIST = "credentials.txt";
const READY_TIMEOUT_MS = 60_000;

/** Where the journeys put their target repos (`docs/agents/run-scenario.md`):
 *  trusted by the harnesses that ask, so no trust dialog stops a node. */
const TRUSTED_DIRS = ["/tmp"];

/** Copy the auth files of every harness this machine has into `demoHome`;
 *  never a link, never a write to `hostHome`. Returns the files staged. */
export function stageHarnessCredentials({ hostHome, demoHome }) {
  return stageCredentials({ harnesses: Object.keys(HARNESS_AUTH), hostHome, demoHome, trustedDirs: TRUSTED_DIRS });
}

/**
 * Start a demo daemon on `port`. `testExt`: the test extension's demo daemon
 * instead of the bare one. `extra`: variables set explicitly on the daemon.
 * `bindAll`: listen on all interfaces, so a sandboxed Run's container reaches it.
 * `build`: run cargo's incremental build of `pdo` first, so a branch that moved
 * since the last build is not tested on a stale binary (false: build only if
 * the binary is missing). Resolves to `{ url, pdo, pid, paths, env }`.
 */
export async function startDemo({ port, testExt = false, bindAll = false, extra = {}, build = true, repoRoot, tmpRoot = "/tmp", env = process.env, log = console.log }) {
  const paths = demoPaths(port, tmpRoot);
  const running = readPid(paths.pidfile);
  if (running && isAlive(running)) {
    throw new Error(`a demo daemon is already running on port ${port} (pid ${running}): make demo-daemon-stop PORT=${port}`);
  }
  if (!(await portFree(port))) throw new Error(`port ${port} is taken: pick another (6400 to 6999)`);

  const pdo = path.join(targetDir(repoRoot, env), "debug", "pdo");
  if (build || !fs.existsSync(pdo)) {
    log(`building ${pdo} (incremental)`);
    execFileSync("cargo", ["build", "-p", "pdo-daemon", "--bin", "pdo"], { cwd: repoRoot, stdio: ["ignore", "inherit", "inherit"] });
  }
  const [command, args, own] = testExt
    ? [testExtensionBinary(repoRoot), ["demo_daemon::demo_daemon", "--exact", "--ignored", "--nocapture"], { PDO_DEMO_PORT: String(port), PDO_DEMO_ROOT: paths.cwd }]
    : [pdo, ["daemon"], {}];

  fs.rmSync(paths.root, { recursive: true, force: true });
  for (const dir of [paths.home, paths.cwd, paths.bin]) fs.mkdirSync(dir, { recursive: true });
  fs.symlinkSync(pdo, path.join(paths.bin, "pdo"));
  const staged = stageHarnessCredentials({ hostHome: env.HOME, demoHome: paths.home });
  fs.writeFileSync(path.join(paths.root, STAGED_LIST), staged.map((file) => `${file}\n`).join(""));

  const daemonEnv = demoEnv({ env, paths, port, extra: { ...extra, ...own }, bind: bindAll ? "0.0.0.0" : "127.0.0.1" });
  const logFile = fs.openSync(paths.log, "a");
  const child = spawn(command, args, { cwd: paths.cwd, env: daemonEnv, detached: true, stdio: ["ignore", logFile, logFile] });
  fs.closeSync(logFile);
  child.unref();
  fs.writeFileSync(paths.pidfile, `${child.pid}\n`);

  const url = `http://127.0.0.1:${port}`;
  try {
    await waitReady(url, child.pid, paths.log);
  } catch (error) {
    const tail = readTail(paths.log);
    stopDemo({ port, tmpRoot, keep: true, log });
    throw new Error(`${error.message}\n--- ${paths.log} (kept) ---\n${tail}`);
  }
  return { url, pdo, pid: child.pid, paths, env: daemonEnv };
}

/**
 * Stop the demo daemon of `port`. Refuses a port without a pidfile. `keep`
 * leaves the demo folder (logs) in place, auth files wiped.
 */
export function stopDemo({ port, tmpRoot = "/tmp", keep = false, log = console.log }) {
  const paths = demoPaths(port, tmpRoot);
  const pid = readPid(paths.pidfile);
  if (pid === null) throw new Error(`no demo daemon on port ${port}: ${paths.pidfile} does not exist (nothing stopped)`);
  let daemonPid = null;
  if (isAlive(pid)) {
    if (!runsUnder(pid, paths.home)) {
      throw new Error(
        `pid ${pid} of ${paths.pidfile} is not the demo daemon (its HOME is not ${paths.home}): nothing stopped. Check it, then remove the pidfile by hand`,
      );
    }
    daemonPid = pid;
  }
  // teardownState runs `tmux -L pdo-<port> kill-server` with this process's
  // environment: without TMUX nor TMUX_PANE, as the daemon was started.
  for (const key of TMUX_VARS) delete process.env[key];
  const authFiles = readLines(path.join(paths.root, STAGED_LIST));
  teardownState({ port, root: paths.root, home: paths.home, daemonPid, authFiles, keep }, log);
  fs.rmSync(paths.pidfile, { force: true });
}

function readPid(file) {
  try {
    const pid = Number(fs.readFileSync(file, "utf8").trim());
    return Number.isInteger(pid) && pid > 0 ? pid : 0;
  } catch {
    return null;
  }
}

function readLines(file) {
  try {
    return fs.readFileSync(file, "utf8").split("\n").filter(Boolean);
  } catch {
    return [];
  }
}

function readTail(file) {
  try {
    return fs.readFileSync(file, "utf8").split("\n").slice(-30).join("\n");
  } catch {
    return "(no log)";
  }
}

/** The process runs under the demo HOME. Without procfs (macOS), the pidfile is trusted. */
function runsUnder(pid, home) {
  let environ;
  try {
    environ = fs.readFileSync(`/proc/${pid}/environ`, "utf8");
  } catch {
    return !fs.existsSync("/proc/self");
  }
  return environ.split("\0").includes(`HOME=${home}`);
}

function targetDir(repoRoot, env) {
  return env.CARGO_TARGET_DIR ? path.resolve(repoRoot, env.CARGO_TARGET_DIR) : path.join(repoRoot, "target");
}

/** The `it` test binary that holds the test extension's demo daemon, built if needed. */
function testExtensionBinary(repoRoot) {
  const out = execFileSync("cargo", ["test", "-p", "pdo-daemon", "--test", "it", "--no-run", "--message-format=json-render-diagnostics"], {
    cwd: repoRoot,
    encoding: "utf8",
    maxBuffer: 256 * 1024 * 1024,
    stdio: ["ignore", "pipe", "inherit"],
  });
  for (const line of out.split("\n")) {
    if (!line.startsWith("{")) continue;
    const message = JSON.parse(line);
    if (message.reason === "compiler-artifact" && message.target?.name === "it" && message.executable) return message.executable;
  }
  throw new Error("cargo built no `it` test binary for pdo-daemon");
}

function portFree(port) {
  return new Promise((resolve) => {
    const server = net.createServer();
    server.once("error", () => resolve(false));
    server.listen(port, "127.0.0.1", () => server.close(() => resolve(true)));
  });
}

async function waitReady(url, pid, logPath) {
  const deadline = Date.now() + READY_TIMEOUT_MS;
  let lastError = null;
  while (Date.now() < deadline) {
    if (!isAlive(pid)) throw new Error(`the demo daemon exited during startup — see ${logPath}`);
    try {
      if ((await fetch(`${url}/pipelines`)).ok) return;
    } catch (error) {
      lastError = error;
    }
    await sleep(250);
  }
  throw new Error(`the demo daemon is not ready after ${READY_TIMEOUT_MS / 1000} s: ${lastError?.message ?? "no answer"}`);
}
