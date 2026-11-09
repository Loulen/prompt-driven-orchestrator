// `make demo-daemon PORT=<p> [TEST_EXT=1] [BIND_ALL=1] [DEMO_ENV='K=V …']` and `make
// demo-daemon-stop PORT=<p> [KEEP=1]`: a PDO daemon for an agentic test,
// sealed off from the user's (lib/daemon.mjs). See docs/agents/run-scenario.md.
//
//   node scripts/demo-daemon/demo-daemon.mjs start <port> [--test-ext] [--bind-all] [--env 'K=V …']
//   node scripts/demo-daemon/demo-daemon.mjs stop <port> [--keep]

import path from "node:path";
import { fileURLToPath } from "node:url";
import { startDemo, stopDemo } from "./lib/daemon.mjs";
import { parsePort } from "./lib/env.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");

/** `--env 'A=1 B=2'` → { A: "1", B: "2" }. */
function parseExtra(text) {
  const extra = {};
  for (const pair of (text ?? "").split(/\s+/).filter(Boolean)) {
    const at = pair.indexOf("=");
    if (at <= 0) throw new Error(`DEMO_ENV: \`${pair}\` is not NAME=value`);
    extra[pair.slice(0, at)] = pair.slice(at + 1);
  }
  return extra;
}

async function main(argv) {
  const [verb, portText, ...rest] = argv;
  const flag = (name) => rest.includes(name);
  const value = (name) => (rest.includes(name) ? rest[rest.indexOf(name) + 1] : undefined);
  if (verb === "start") {
    const port = parsePort(portText, process.env);
    const demo = await startDemo({ port, testExt: flag("--test-ext"), bindAll: flag("--bind-all"), extra: parseExtra(value("--env")), repoRoot });
    const unset = Object.keys(process.env)
      .filter((key) => key.startsWith("PDO_"))
      .map((key) => `-u ${key} `)
      .join("");
    const extras = `${flag("--test-ext") ? ", with the test extension" : ""}${flag("--bind-all") ? ", on all interfaces (sandboxed Runs reach it)" : ""}`;
    console.log(`demo daemon up on port ${port}${extras} (pid ${demo.pid}, HOME ${demo.paths.home}, log ${demo.paths.log})`);
    console.log(`PDO_DAEMON_URL=${demo.url}`);
    console.log(`PDO=${demo.pdo}`);
    console.log(`Drive it:  env ${unset}PDO_DAEMON_URL=${demo.url} ${demo.pdo} run list`);
    console.log(`Stop it:   make demo-daemon-stop PORT=${port}`);
  } else if (verb === "stop") {
    const port = parsePort(portText, process.env);
    stopDemo({ port, keep: flag("--keep") });
    console.log(`demo daemon of port ${port} stopped (tmux socket pdo-${port} killed)`);
  } else {
    throw new Error("usage: demo-daemon.mjs start|stop <port>");
  }
}

main(process.argv.slice(2)).catch((error) => {
  console.error(`demo-daemon: ${error.message}`);
  process.exit(1);
});
