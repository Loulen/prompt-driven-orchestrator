// The auth files of the harnesses a scene plays LIVE, staged into the demo HOME
// for the recording and wiped afterwards (ADR-0074 §4). One entry per harness:
// `copy` lists files taken verbatim from the user's home, `derive` writes files
// built from the user's (e.g. the account block of `~/.claude.json`, never the
// whole file, which carries the user's projects and MCP servers). Every file
// either step writes is recorded BEFORE it is written, so the exit guard knows
// what to wipe even after an interruption in the middle.

import fs from "node:fs";
import path from "node:path";

export const HARNESS_AUTH = {
  claude: {
    copy: [".claude/.credentials.json"],
    derive({ hostHome, demoHome, trustedDirs, record }) {
      let host = {};
      try {
        host = JSON.parse(fs.readFileSync(path.join(hostHome, ".claude.json"), "utf8"));
      } catch {
        // No host config: the credentials file alone may be enough.
      }
      const config = {
        hasCompletedOnboarding: true,
        ...(host.oauthAccount ? { oauthAccount: host.oauthAccount } : {}),
        ...(host.userID ? { userID: host.userID } : {}),
        projects: Object.fromEntries(trustedDirs.map((dir) => [dir, { hasTrustDialogAccepted: true }])),
      };
      const target = path.join(demoHome, ".claude.json");
      record(target);
      writePrivate(target, `${JSON.stringify(config, null, 2)}\n`);
      // Not a secret: the settings PDO's own sandbox staging poses so an
      // unwatched `--dangerously-skip-permissions` session never blocks.
      const settings = path.join(demoHome, ".claude", "settings.json");
      fs.mkdirSync(path.dirname(settings), { recursive: true });
      fs.writeFileSync(settings, `${JSON.stringify({ skipDangerousModePermissionPrompt: true }, null, 2)}\n`);
    },
  },
};

function writePrivate(target, content) {
  fs.mkdirSync(path.dirname(target), { recursive: true, mode: 0o700 });
  fs.writeFileSync(target, content, { mode: 0o600 });
}

/**
 * Stage the auth of `harnesses` from `hostHome` into `demoHome`. `onStaged` is
 * called with each path BEFORE its content is written.
 */
export function stageCredentials({ harnesses, hostHome, demoHome, trustedDirs, onStaged }) {
  const staged = [];
  const record = (file) => {
    staged.push(file);
    onStaged?.(file);
  };
  for (const harness of harnesses) {
    const auth = HARNESS_AUTH[harness];
    if (!auth) throw new Error(`no auth recipe for live harness "${harness}" (add it to HARNESS_AUTH in lib/credentials.mjs)`);
    for (const rel of auth.copy) {
      const from = path.join(hostHome, rel);
      if (!fs.existsSync(from)) continue;
      const to = path.join(demoHome, rel);
      record(to);
      writePrivate(to, fs.readFileSync(from));
    }
    auth.derive?.({ hostHome, demoHome, trustedDirs, record });
  }
  return staged;
}

/** Delete every staged auth file. Synchronous: it runs from the exit guard. */
export function wipeCredentials(files) {
  const left = [];
  for (const file of files) {
    try {
      fs.rmSync(file, { force: true });
    } catch {
      left.push(file);
    }
    if (fs.existsSync(file)) left.push(file);
  }
  return left;
}
