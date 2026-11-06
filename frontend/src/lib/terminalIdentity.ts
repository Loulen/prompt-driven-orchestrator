/**
 * #1038 — a terminal's identity: what you are looking at, named by the Run
 * first, then what runs inside it (a node, « Manager », « Shell »). The tmux
 * session name (`pdo-<run_id>-<node_id>-iter-N`) is a technical id, never the
 * title — see CONTEXT.md « Identité du terminal ».
 *
 * A Run should always have a name, but one created auto-named with no Manager
 * can still be nameless (#1039): the identity then falls back to the subject.
 */
export const IDENTITY_SEPARATOR = " › ";

export function terminalTitle(runName: string | null | undefined, subject: string): string {
  const run = runName?.trim();
  return run ? `${run}${IDENTITY_SEPARATOR}${subject}` : subject;
}

/** Tooltip of the session chip: the tmux name, and what a click does. */
export function sessionChipLabel(session: string, feedback: "copied" | "failed" | null): string {
  if (feedback === "copied") return "Session name copied";
  if (feedback === "failed") return `Copy failed — tmux session: ${session}`;
  return `tmux session: ${session} · click to copy (tmux attach -t …)`;
}
