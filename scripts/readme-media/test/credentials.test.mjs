import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { stageCredentials, wipeCredentials } from "../lib/credentials.mjs";

test("claude's demo settings name the model on the terminal's status line, and are not staged as a secret", (t) => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "readme-media-credentials-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const hostHome = path.join(root, "host");
  const demoHome = path.join(root, "demo");
  fs.mkdirSync(path.join(hostHome, ".claude"), { recursive: true });
  fs.writeFileSync(path.join(hostHome, ".claude", ".credentials.json"), "{}");

  const staged = stageCredentials({ harnesses: ["claude"], hostHome, demoHome, trustedDirs: [] });
  const settingsFile = path.join(demoHome, ".claude", "settings.json");
  const settings = JSON.parse(fs.readFileSync(settingsFile, "utf8"));
  assert.equal(settings.skipDangerousModePermissionPrompt, true);
  // Claude Code pipes the session JSON to the command: it prints what really runs.
  const line = execFileSync("sh", ["-c", settings.statusLine.command], {
    input: JSON.stringify({ model: { id: "claude-opus-5-5", display_name: "Opus 5.5" } }),
    encoding: "utf8",
  });
  assert.equal(line, "Opus 5.5 · claude-opus-5-5");

  // Auth files are recorded for the wipe; the settings file is not a secret.
  assert.ok(!staged.includes(settingsFile));
  assert.deepEqual(wipeCredentials(staged), []);
  assert.ok(fs.existsSync(settingsFile));
});
