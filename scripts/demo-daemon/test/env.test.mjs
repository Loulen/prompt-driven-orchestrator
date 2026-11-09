// The environment of the demo daemon (`make demo-daemon`): built from the
// caller's, never inherited as is. Pure function, no daemon.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { demoEnv, demoPaths, parsePort } from "../lib/env.mjs";

const paths = demoPaths(6391, "/tmp");

/** A caller running inside a PDO node of the user's daemon, under tmux. */
const caller = {
  HOME: "/home/me",
  PATH: "/home/me/.cargo/bin:/usr/bin",
  SHELL: "/bin/bash",
  PDO_PORT: "5272",
  PDO_DAEMON_URL: "http://localhost:5272",
  PDO_TOKEN: "pdo_node_secret",
  PDO_RUN_ID: "20261031-000000-aaaaaaa",
  PDO_NODE_ID: "XBG5Cxkn",
  PDO_DAEMON_NO_CLEANUP: "1",
  TMUX: "/tmp/tmux-1000/pdo-5272,1,0",
  TMUX_PANE: "%140",
  TMUX_TMPDIR: "/run/user/1000",
  INVOCATION_ID: "0123456789abcdef",
  JOURNAL_STREAM: "8:12345",
  NOTIFY_SOCKET: "/run/systemd/notify",
  CLAUDE_CONFIG_DIR: "/home/me/.claude",
  XDG_CONFIG_HOME: "/home/me/.config",
  XDG_DATA_HOME: "/home/me/.local/share",
};

describe("demo daemon paths", () => {
  test("everything lives under /tmp/pdo-demo-<port>, the HOME included", () => {
    assert.equal(paths.root, "/tmp/pdo-demo-6391");
    assert.equal(paths.home, "/tmp/pdo-demo-6391/home");
    assert.ok(paths.cwd.startsWith(`${paths.home}/`));
    assert.equal(paths.pidfile, "/tmp/pdo-demo-6391/daemon.pid");
    assert.equal(paths.log, "/tmp/pdo-demo-6391/daemon.log");
    assert.equal(paths.bin, "/tmp/pdo-demo-6391/bin");
  });
});

describe("demo daemon environment", () => {
  const env = demoEnv({ env: caller, paths, port: 6391 });

  test("no PDO_* of the caller survives: only the demo's own", () => {
    const pdo = Object.fromEntries(Object.entries(env).filter(([key]) => key.startsWith("PDO_")));
    assert.deepEqual(pdo, {
      PDO_PORT: "6391",
      PDO_BIND: "127.0.0.1",
      PDO_UPDATE_CHECK: "off",
      PDO_PRICE_SYNC: "off",
      PDO_HARNESS_PROBE_PATH: `${paths.bin}:${caller.PATH}`,
    });
  });

  test("not in nested mode: neither PDO_NODE_ID nor PDO_DAEMON_NO_CLEANUP", () => {
    assert.equal(env.PDO_NODE_ID, undefined);
    assert.equal(env.PDO_DAEMON_NO_CLEANUP, undefined);
  });

  test("no TMUX nor TMUX_PANE of the caller: the demo tmux server is not nested in the caller's", () => {
    assert.equal(env.TMUX, undefined);
    assert.equal(env.TMUX_PANE, undefined);
    // Where the sockets live stays the caller's: `tmux -L pdo-<port>` finds the demo's.
    assert.equal(env.TMUX_TMPDIR, caller.TMUX_TMPDIR);
  });

  test("no mark of the caller's service manager: the demo daemon is not a supervised service", () => {
    assert.equal(env.INVOCATION_ID, undefined);
    assert.equal(env.JOURNAL_STREAM, undefined);
    assert.equal(env.NOTIFY_SOCKET, undefined);
  });

  test("a fresh HOME, and no variable pointing a harness at the real config", () => {
    assert.equal(env.HOME, paths.home);
    assert.equal(env.CLAUDE_CONFIG_DIR, undefined);
    assert.equal(env.XDG_CONFIG_HOME, undefined);
    assert.equal(env.XDG_DATA_HOME, undefined);
  });

  test("the toolchains stay the real ones, defaulted from the real HOME", () => {
    assert.equal(env.RUSTUP_HOME, "/home/me/.rustup");
    assert.equal(env.CARGO_HOME, "/home/me/.cargo");
    assert.equal(env.COREPACK_HOME, "/home/me/.cache/node/corepack");
  });

  test("toolchain locations the caller already set are kept", () => {
    const pinned = demoEnv({
      env: { ...caller, RUSTUP_HOME: "/opt/rustup", CARGO_HOME: "/opt/cargo", XDG_CACHE_HOME: "/var/cache/me" },
      paths,
      port: 6391,
    });
    assert.equal(pinned.RUSTUP_HOME, "/opt/rustup");
    assert.equal(pinned.CARGO_HOME, "/opt/cargo");
    assert.equal(pinned.COREPACK_HOME, "/var/cache/me/node/corepack");
    assert.equal(pinned.XDG_CACHE_HOME, undefined);
    assert.equal(demoEnv({ env: { ...caller, COREPACK_HOME: "/opt/corepack" }, paths, port: 6391 }).COREPACK_HOME, "/opt/corepack");
  });

  test("the branch's pdo comes first on the PATH; the rest of the caller's is kept", () => {
    assert.equal(env.PATH, `${paths.bin}:${caller.PATH}`);
    assert.equal(env.SHELL, "/bin/bash");
  });

  test("extra variables are set explicitly, after the purge", () => {
    const extra = demoEnv({ env: caller, paths, port: 6391, extra: { PDO_TEST_INTEROP: "1" } });
    assert.equal(extra.PDO_TEST_INTEROP, "1");
    assert.equal(extra.PDO_TOKEN, undefined);
  });

  test("a sandboxed journey binds the daemon beyond loopback, so its containers reach it", () => {
    assert.equal(demoEnv({ env: caller, paths, port: 6391, bind: "0.0.0.0" }).PDO_BIND, "0.0.0.0");
    assert.throws(() => demoEnv({ env: caller, paths, port: 6391, bind: "everywhere" }), /BIND/);
  });

  test("extra variables may not undo the isolation", () => {
    for (const key of ["HOME", "PDO_PORT", "PDO_NODE_ID", "PDO_DAEMON_NO_CLEANUP", "TMUX"]) {
      assert.throws(() => demoEnv({ env: caller, paths, port: 6391, extra: { [key]: "x" } }), new RegExp(key));
    }
  });

  test("the caller's environment is not modified", () => {
    assert.equal(caller.PDO_TOKEN, "pdo_node_secret");
    assert.equal(caller.HOME, "/home/me");
  });
});

describe("the port", () => {
  test("a number is accepted", () => {
    assert.equal(parsePort("6391", {}), 6391);
  });

  test("a missing or malformed port is refused", () => {
    assert.throws(() => parsePort(undefined, {}), /PORT=/);
    assert.throws(() => parsePort("", {}), /PORT=/);
    assert.throws(() => parsePort("63a1", {}), /PORT=/);
    assert.throws(() => parsePort("70000", {}), /PORT=/);
  });

  test("the user's ports are refused, the caller's daemon's included", () => {
    assert.throws(() => parsePort("5272", {}), /user's/);
    assert.throws(() => parsePort("5172", {}), /user's/);
    assert.throws(() => parsePort("6555", { PDO_PORT: "6555" }), /user's/);
  });

  test("the refusal suits both start and stop: it never asks to pick a free port", () => {
    assert.throws(() => parsePort("5272", {}), (err) => /never runs there/.test(err.message) && !/pick another/.test(err.message));
  });
});
