// Helper of the settings scenes (triggers, profiles, skills — #857), which
// play no live agent. Two locks keep it that way:
//
//   - in the page, every request that could start an agent session (a new
//     run, a trigger's « Run now », a retry, a resume) is aborted before it
//     reaches the demo daemon, so a stray click cannot launch one;
//   - after the variant, the demo tmux socket must hold no session.
//
// Files starting with `_` are helpers, not scenes (lib/scenes.mjs).

import { execFileSync } from "node:child_process";

const LAUNCHES = [/\/runs$/, /\/triggers\/[^/]+\/fire$/, /\/runs\/[^/]+\/(retry|resume|nodes\/[^/]+\/(retry|restart))/];

/** Abort, in `page`, every POST that could start an agent session. */
export async function forbidAgentLaunch(page) {
  await page.route(
    (url) => LAUNCHES.some((re) => re.test(url.pathname)),
    (route) => (route.request().method() === "POST" ? route.abort("blockedbyclient") : route.fallback()),
  );
}

/** Throw if the demo instance's tmux socket holds any session. */
export function assertNoAgent(instance) {
  let sessions = "";
  try {
    sessions = execFileSync("tmux", ["-L", instance.tmuxSocket, "list-sessions", "-F", "#{session_name}"], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    }).trim();
  } catch {
    return; // no tmux server on the demo socket: nothing was launched
  }
  if (sessions) throw new Error(`an agent session was launched during a settings scene: ${sessions.split("\n").join(", ")}`);
}

/** The runs list is filled (the rail loads a beat after the page): never film « No runs yet ». */
export async function waitForRuns(page) {
  await page.getByTestId("run-display-label").first().waitFor({ timeout: 30_000 });
}
