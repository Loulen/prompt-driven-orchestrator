import type { Trigger } from "../../shared/generated/Trigger";
import type { NewRunPrefill } from "../../shared/extensions/contract";

export type { NewRunPrefill };

/** The prefilled fields of a `run` intent, `null` for a plain open. */
export function prefillOf(intent: OpenIntent): NewRunPrefill | null {
  return intent.kind === "run" && intent.prefill ? intent.prefill : null;
}

/**
 * How the always-mounted modal should open (#386). Because the modal is never
 * unmounted (`if (!open) return null` below), its useState survives a close, so
 * the open intent drives a one-shot reset on every reopen — a stale mode /
 * trigger can no longer leak into a fresh open.
 *
 * - `run` — a plain New Run; with `prefill`, opened prefilled by an extension
 *   (API 0.22, `NewRunPrefill`). An empty prefill opens it as the core's button does.
 * - `new-trigger` — Trigger mode, blank, POSTs a new trigger.
 * - `edit-trigger` — Trigger mode bound to `trigger`; submitting PATCHes it (#162).
 *
 * There is deliberately no "run-from-trigger" variant: since ADR-0027, "Run now"
 * is a real fire (`POST /triggers/{id}/fire`), not a prefilled modal.
 */
export type OpenIntent =
  | { kind: "run"; prefill?: NewRunPrefill }
  | { kind: "new-trigger" }
  | { kind: "edit-trigger"; trigger: Trigger };

// Module constant: stabilises the `[open, openIntent]` dependency and serves as
// the default on both sides (prop + destructuring) so a plain open doesn't mint
// a new identity every render. It lives in a `.ts` module (no component here), so
// the Fast-Refresh rule has nothing to say about it.
export const RUN_INTENT: OpenIntent = { kind: "run" };
