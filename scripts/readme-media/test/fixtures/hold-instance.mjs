// Test helper: start a demo instance (live `claude`, auth from a fake home),
// print its state, then end the way the test asks — hang (to be interrupted or
// killed), or crash with an uncaught error. `stubborn` stands in for a real
// `claude`: its agent outlives the hangup a couple of seconds, and on its way
// out writes under the demo HOME (what recreated the root after #858's teardown).
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { DemoInstance } from "../../lib/demo-instance.mjs";

const [tmpRoot, hostHome, mode] = process.argv.slice(2);
const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "..");
const instance = new DemoInstance({ repoRoot, tmpRoot, hostHome, liveHarnesses: ["claude"], log: () => {} });
await instance.start();
// Stand-in for a live agent: a session on the demo tmux socket, with the demo
// HOME as the daemon gives it to its sessions.
const agent =
  mode === "stubborn"
    ? `trap 'sleep 2; mkdir -p "$HOME/.claude/late-write"; exit 0' HUP TERM; echo $$ > "${instance.state.root}/agent.pid"; while :; do sleep 0.2; done`
    : "sleep 600";
execFileSync("tmux", ["-L", instance.tmuxSocket, "new-session", "-d", "-s", "pdo-demo-agent", "-e", `HOME=${instance.home}`, "bash", "-c", agent], {
  env: { ...process.env, HOME: instance.home },
});
let agentPid = null;
if (mode === "stubborn") {
  const pidFile = path.join(instance.state.root, "agent.pid");
  while (!fs.existsSync(pidFile) || !fs.readFileSync(pidFile, "utf8").trim()) await new Promise((r) => setTimeout(r, 50));
  agentPid = Number(fs.readFileSync(pidFile, "utf8"));
}
console.log(JSON.stringify({ ...instance.state, agentPid }));
if (mode === "crash") {
  setTimeout(() => {
    throw new Error("scene crashed");
  }, 200);
}
setInterval(() => {}, 1000);
