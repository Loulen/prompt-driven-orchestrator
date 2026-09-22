// Test helper: start a demo instance (live `claude`, auth from a fake home),
// print its state, then end the way the test asks — hang (to be interrupted or
// killed), or crash with an uncaught error.
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { DemoInstance } from "../../lib/demo-instance.mjs";

const [tmpRoot, hostHome, mode] = process.argv.slice(2);
const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "..");
const instance = new DemoInstance({ repoRoot, tmpRoot, hostHome, liveHarnesses: ["claude"], log: () => {} });
await instance.start();
// Stand-in for a live agent: a session on the demo tmux socket.
execFileSync("tmux", ["-L", instance.tmuxSocket, "new-session", "-d", "-s", "pdo-demo-agent", "sleep 600"]);
console.log(JSON.stringify(instance.state));
if (mode === "crash") {
  setTimeout(() => {
    throw new Error("scene crashed");
  }, 200);
}
setInterval(() => {}, 1000);
