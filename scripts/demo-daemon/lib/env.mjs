// The demo daemon's paths and environment (`make demo-daemon`): pure, so the
// isolation from the user's daemon is tested without starting anything.
//
// The environment is the caller's, minus what would tie the demo daemon to the
// caller's instance: every PDO_* (the caller may itself run inside a PDO node:
// its PDO_PORT, PDO_TOKEN, and PDO_NODE_ID, which would turn on the nested
// mode), TMUX and TMUX_PANE (the demo tmux server must not nest in the
// caller's; TMUX_TMPDIR stays, so `tmux -L pdo-<port>` finds it), and
// the variables that point a harness at the real config folders. The HOME is
// a fresh one; the toolchains (rustup, cargo, corepack) stay the real ones, so
// a build or a `pnpm` inside a demo node does not download them again.

import path from "node:path";
import { RESERVED_PORTS } from "../../readme-media/lib/demo-instance.mjs";

/** The caller's tmux: the demo tmux server must not nest in it. */
export const TMUX_VARS = ["TMUX", "TMUX_PANE"];

/** The service manager's marks: inherited from a caller that systemd started (the
 *  user's daemon, its nodes), they would make the demo daemon read itself as a
 *  supervised service (CORE-232: « Method: Unknown · systemd service »). */
export const SUPERVISOR_VARS = ["INVOCATION_ID", "JOURNAL_STREAM", "NOTIFY_SOCKET"];

/** What `extra` may not set: the isolation itself (HOME, port, nested mode). */
const NOT_EXTRA = ["HOME", "PATH", "PDO_PORT", "PDO_BIND", "PDO_NODE_ID", "PDO_DAEMON_NO_CLEANUP", "PDO_TOKEN", ...TMUX_VARS];

/** Removed from the caller's environment on top of PDO_* and TMUX_VARS: each
 *  would lead a harness (or its config) back to the real home. */
const REAL_CONFIG_VARS = ["CLAUDE_CONFIG_DIR", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME"];

/** Where a demo daemon on `port` keeps everything: one folder, removed by the stop. */
export function demoPaths(port, tmpRoot = "/tmp") {
  const root = path.join(tmpRoot, `pdo-demo-${port}`);
  const home = path.join(root, "home");
  return {
    root,
    home,
    // Where an installed PDO runs (`make install`: ~/.pdo/app): the event log
    // `.pdo/pdo.db` follows the working directory.
    cwd: path.join(home, ".pdo", "app"),
    // A `pdo` pointing at the branch's binary, first on the nodes' PATH.
    bin: path.join(root, "bin"),
    pidfile: path.join(root, "daemon.pid"),
    log: path.join(root, "daemon.log"),
  };
}

/** The port of `PORT=`: a number, never one of the user's. */
export function parsePort(text, env) {
  if (!/^\d+$/.test(text ?? "") || Number(text) < 1024 || Number(text) > 65535) {
    throw new Error(`PORT=<port> is required, a number between 1024 and 65535 (got ${JSON.stringify(text ?? "")})`);
  }
  const port = Number(text);
  const callerPort = env.PDO_PORT ?? portOf(env.PDO_DAEMON_URL);
  if (RESERVED_PORTS.has(port) || String(port) === String(callerPort)) {
    throw new Error(`port ${port} is one of the user's daemon ports: a demo daemon never runs there`);
  }
  return port;
}

function portOf(url) {
  try {
    return new URL(url).port || null;
  } catch {
    return null;
  }
}

/**
 * The demo daemon's environment, from the caller's `env` (left untouched).
 * `extra` holds variables the caller asks for explicitly (`DEMO_ENV=`), set
 * after the purge; it may not undo the isolation (NOT_EXTRA).
 */
export function demoEnv({ env, paths, port, extra = {} }) {
  const refused = Object.keys(extra).filter((key) => NOT_EXTRA.includes(key));
  if (refused.length > 0) throw new Error(`DEMO_ENV may not set ${refused.join(", ")}: the demo daemon's isolation sets them`);
  const realHome = env.HOME;
  const cacheHome = env.XDG_CACHE_HOME ?? path.join(realHome, ".cache");
  const out = {};
  for (const [key, value] of Object.entries(env)) {
    if (
      key.startsWith("PDO_") ||
      TMUX_VARS.includes(key) ||
      REAL_CONFIG_VARS.includes(key) ||
      SUPERVISOR_VARS.includes(key)
    )
      continue;
    out[key] = value;
  }
  const searchPath = `${paths.bin}:${env.PATH ?? ""}`;
  Object.assign(out, {
    HOME: paths.home,
    PATH: searchPath,
    RUSTUP_HOME: env.RUSTUP_HOME ?? path.join(realHome, ".rustup"),
    CARGO_HOME: env.CARGO_HOME ?? path.join(realHome, ".cargo"),
    COREPACK_HOME: env.COREPACK_HOME ?? path.join(cacheHome, "node", "corepack"),
    PDO_PORT: String(port),
    PDO_BIND: "127.0.0.1",
    PDO_UPDATE_CHECK: "off",
    PDO_PRICE_SYNC: "off",
    // The nodes' PATH (ADR-0055 reads it from an interactive shell, which under
    // the fresh HOME has no rc file and would lose the user's managers): the
    // branch's `pdo` first, then the caller's tools, harnesses included.
    PDO_HARNESS_PROBE_PATH: searchPath,
    ...extra,
  });
  return out;
}
