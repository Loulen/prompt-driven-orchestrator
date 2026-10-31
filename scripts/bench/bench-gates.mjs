#!/usr/bin/env node
// `make bench-gates REF="<ref> [<ref> …]"`: the dev loop measured, ref by ref, in
// a throwaway worktree (removed even on failure or Ctrl-C): cold build, build
// with a warm sccache, time to the first structural failure (a 401-line file),
// the scoped gate on a front-only diff, the full suite. A gate missing at a ref
// is « n/a ». Writes one markdown table, one column per ref.
//
//   OUT=<file>        where the table goes (default target/bench-gates/bench-<date>.md)
//   MIN_FREE_GB=<n>   free disk needed before each measure (default 25)
//
// The output of every measure is kept in target/bench-gates/<ref>/<measure>.log.

import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { runBench } from "./lib/bench.mjs";
import { renderNotes, renderTable } from "./lib/table.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const outDir = path.join(root, "target/bench-gates");
const refs = (process.env.REF ?? "").split(/\s+/).filter(Boolean);
if (!refs.length) {
  console.error('bench-gates: REF="<ref> [<ref> …]" is required, e.g. make bench-gates REF="40e82f49 HEAD"');
  process.exit(2);
}
const minFreeGb = Number(process.env.MIN_FREE_GB || 25);
const cachePort = String(4300 + Math.floor(Math.random() * 600));
const slug = (ref) => ref.replace(/[^A-Za-z0-9._-]+/g, "-");

const git = (...args) => execFileSync("git", args, { cwd: root, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
const tryRun = (cmd, args, env) => {
  try {
    return execFileSync(cmd, args, { encoding: "utf8", env, stdio: ["ignore", "pipe", "ignore"] }).trim();
  } catch {
    return null;
  }
};

// The environment of the measures: the caller's, without what would steer them
// elsewhere (a PDO node's PDO_*, a cache wrapper or target dir of the caller's,
// the MAKEFLAGS that would hand REF= and OUT= to the measured `make`).
const KEEP_PDO = new Set(["PDO_FRONTEND_DIST", "PDO_SKIP_FRONTEND_BUILD"]);
const baseEnv = Object.fromEntries(
  Object.entries(process.env).filter(
    ([k]) => !(k.startsWith("PDO_") && !KEEP_PDO.has(k)) && !k.startsWith("SCCACHE_") && !["RUSTC_WRAPPER", "CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR", "MAKEFLAGS", "MFLAGS", "MAKELEVEL", "MAKEOVERRIDES"].includes(k),
  ),
);
let cacheDir = null;
const cacheEnv = () => ({ ...baseEnv, RUSTC_WRAPPER: "sccache", SCCACHE_DIR: cacheDir, SCCACHE_SERVER_PORT: cachePort });

let current = null; // the measure in flight, killed on Ctrl-C
let worktree = null;

// Never throws: a leftover is reported, so the cleanup still reaches the next step.
function removeWorktree(dir) {
  if (!dir) return;
  tryRun("git", ["-C", root, "worktree", "remove", "--force", dir], process.env);
  try {
    fs.rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 500 });
  } catch (err) {
    console.error(`bench-gates: could not remove ${dir}: ${err.message}; remove it by hand`);
  }
  tryRun("git", ["-C", root, "worktree", "prune"], process.env);
  if (worktree === dir) worktree = null;
}

function stopCache() {
  if (!cacheDir) return;
  tryRun("sccache", ["--stop-server"], cacheEnv());
  try {
    fs.rmSync(cacheDir, { recursive: true, force: true, maxRetries: 5, retryDelay: 500 });
  } catch (err) {
    console.error(`bench-gates: could not remove ${cacheDir}: ${err.message}; remove it by hand`);
  }
  cacheDir = null;
}

/** Stops the measure in flight (its whole process group) and waits for it, SIGKILL after 10 s. */
async function stopCurrent() {
  const child = current;
  if (!child) return;
  const closed = new Promise((resolve) => child.once("close", resolve));
  const kill = (sig) => {
    try {
      process.kill(-child.pid, sig);
    } catch {}
  };
  kill("SIGTERM");
  const timer = setTimeout(() => kill("SIGKILL"), 10_000);
  await closed;
  clearTimeout(timer);
}

let stopping = false;
for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"]) {
  process.on(signal, async () => {
    if (stopping) return;
    stopping = true;
    await stopCurrent();
    stopCache();
    removeWorktree(worktree);
    console.error(`bench-gates: ${signal}, worktree and cache removed`);
    process.exit(130);
  });
}

/** Runs `line` under bash in `dir`, its output appended to `logFile`; resolves `{ code, output }`. */
function bash(dir, line, env, logFile) {
  return new Promise((resolve) => {
    const child = spawn("bash", ["-c", line], { cwd: dir, env, detached: true, stdio: ["ignore", "pipe", "pipe"] });
    current = child;
    const log = logFile ? fs.createWriteStream(logFile, { flags: "a" }) : null;
    let output = "";
    const keep = (chunk) => {
      log?.write(chunk);
      output = (output + chunk).slice(-4_000_000);
    };
    child.stdout.on("data", keep);
    child.stderr.on("data", keep);
    child.on("error", (err) => resolve({ code: 127, output: `${output}\n${err.message}` }));
    child.on("close", (code, signal) => {
      current = null;
      log?.end();
      resolve({ code: code ?? (signal ? 128 : 1), output });
    });
  });
}

function cacheStats() {
  const stats = tryRun("sccache", ["--show-stats"], cacheEnv()) ?? "";
  tryRun("sccache", ["--zero-stats"], cacheEnv());
  const num = (name) => Number(new RegExp(`^${name}\\s+(\\d+)`, "m").exec(stats)?.[1] ?? NaN);
  const hits = num("Cache hits");
  const misses = num("Cache misses");
  return Number.isNaN(hits) ? "sccache stats unavailable" : `sccache ${hits} hits / ${misses} misses`;
}

function opsFor(ref) {
  const logs = path.join(outDir, slug(ref));
  return {
    now: () => new Date().toISOString().replace(/\.\d+Z$/, "Z"),
    load: () => os.loadavg()[0],
    freeGb: () => {
      const s = fs.statfsSync(worktree ?? root);
      return (s.bavail * s.bsize) / 1e9;
    },
    sccache: () => tryRun("sccache", ["--version"], process.env),
    readMakefile: (dir) => fs.readFileSync(path.join(dir, "Makefile"), "utf8"),
    addWorktree: async (r) => {
      const sha = git("rev-parse", "--verify", `${r}^{commit}`);
      fs.rmSync(logs, { recursive: true, force: true });
      fs.mkdirSync(logs, { recursive: true });
      const dir = fs.mkdtempSync(path.join(os.tmpdir(), `pdo-bench-${sha.slice(0, 8)}-`));
      worktree = dir;
      try {
        git("worktree", "add", "--detach", dir, sha);
      } catch (err) {
        removeWorktree(dir);
        throw err;
      }
      return { dir, sha };
    },
    removeWorktree: async (dir) => removeWorktree(dir),
    startCache: async () => {
      cacheDir = fs.mkdtempSync(path.join(os.tmpdir(), "pdo-bench-sccache-"));
      tryRun("sccache", ["--start-server"], cacheEnv());
      tryRun("sccache", ["--zero-stats"], cacheEnv());
    },
    stopCache: async () => stopCache(),
    cacheStats,
    shell: async (dir, line) => (await bash(dir, line, baseEnv, path.join(logs, "setup.log"))).code,
    run: async (dir, m) => {
      const t0 = process.hrtime.bigint();
      const { code, output } = await bash(dir, m.command, m.wrapped ? cacheEnv() : baseEnv, path.join(logs, `${m.id}.log`));
      return { code, output, seconds: Number(process.hrtime.bigint() - t0) / 1e9 };
    },
  };
}

const benches = [];
let failed = false;
for (const ref of refs) {
  console.log(`==> bench-gates ${ref}`);
  try {
    benches.push(await runBench(ref, { ops: opsFor(ref), minFreeGb }));
  } catch (err) {
    failed = true;
    console.error(`bench-gates ${ref}: ${err.message} (worktree removed)`);
  }
}
if (benches.length) {
  const machine = { cpus: os.cpus().length, os: `${os.type()} ${os.release()}`, sccache: tryRun("sccache", ["--version"], process.env) };
  const markdown = `${renderTable(benches)}\n\n${renderNotes(benches, machine)}\n`;
  const out = process.env.OUT ? path.resolve(root, process.env.OUT) : path.join(outDir, `bench-${new Date().toISOString().slice(0, 16).replace(/:/g, "")}.md`);
  fs.mkdirSync(path.dirname(out), { recursive: true });
  fs.writeFileSync(out, markdown);
  console.log(`\n${markdown}\nTable written to ${path.relative(root, out)}; logs in ${path.relative(root, outDir)}/`);
}
process.exit(failed ? 1 : 0);
