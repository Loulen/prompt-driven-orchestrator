/**
 * A gesture the daemon refused (`403` `operation_refused`, CORE-42): the UI
 * greys what it knows to be refused (CORE-205), but a right can change
 * between the question and the click. A gesture that otherwise reports
 * nothing says so in a toast, with the sentence for a person the refusal
 * carries ({@link refusalSentenceOf}), never the technical reason.
 */

import { ApiError, refusalSentenceOf } from "../http/client";
import { toast } from "../ui/toast";

/** The slug of every refusal by the Autorisation d'opération. */
export const OPERATION_REFUSED = "operation_refused";

/** Whether `error` is the daemon refusing the gesture. */
export function isRefusal(error: unknown): error is ApiError {
  return (
    error instanceof ApiError &&
    error.status === 403 &&
    (error.body as { error?: unknown } | null | undefined)?.error === OPERATION_REFUSED
  );
}

/** The sentence for a person of `error` when it is a refusal; `null` otherwise. */
export function refusalSentence(error: unknown): string | null {
  return isRefusal(error) ? refusalSentenceOf(error.body) : null;
}

/** Show the refusal of a gesture, if `error` is one; anything else stays silent. */
export function toastRefusal(error: unknown): void {
  const sentence = refusalSentence(error);
  if (sentence !== null) toast(sentence, "error");
}
