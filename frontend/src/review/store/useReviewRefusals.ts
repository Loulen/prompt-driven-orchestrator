/**
 * Which gestures on a Run's review comments the caller may not make, and why
 * (CORE-205): asked once for the page (`POST /authorize`), so a refused Send,
 * Resolve or Reopen stays on screen, greyed, its tooltip saying the refusal's
 * sentence.
 *
 * The daemon's Operations: a send is `review_comment` × `create` on the Run;
 * a resolution or a reopening is `review_comment` × `resolve` / `reopen` on
 * the comment, aimed `<run>/<comment>`. Placed by the daemon in the Run's
 * Projet.
 */

import { useMemo } from "react";
import { useRefusals } from "../../shared/authorization/useRefusals";
import type { OperationQuestion } from "../../shared/authorization/api";

/** Why the caller may not make each gesture on the comments, absent when allowed. */
export interface ReviewRefusals {
  send?: string;
  resolveOf: (commentId: string) => string | undefined;
  reopenOf: (commentId: string) => string | undefined;
}

function onComment(action: "resolve" | "reopen", runId: string, commentId: string): OperationQuestion {
  return { resource: "review_comment", verb: { action }, target: `${runId}/${commentId}` };
}

/** The refusals of the comments `commentIds` of the Run `runId`, and of a send on it. */
export function useReviewRefusals(runId: string, commentIds: readonly string[]): ReviewRefusals {
  const idsKey = JSON.stringify(commentIds);
  const operations = useMemo(() => {
    const ids = JSON.parse(idsKey) as string[];
    return [
      { resource: "review_comment", verb: "create", target: runId } satisfies OperationQuestion,
      ...ids.flatMap((id) => [onComment("resolve", runId, id), onComment("reopen", runId, id)]),
    ];
  }, [runId, idsKey]);
  const refusals = useRefusals(operations);
  return useMemo(() => {
    const ids = JSON.parse(idsKey) as string[];
    const resolve = new Map<string, string | undefined>();
    const reopen = new Map<string, string | undefined>();
    ids.forEach((id, i) => {
      resolve.set(id, refusals[1 + 2 * i]);
      reopen.set(id, refusals[2 + 2 * i]);
    });
    return { send: refusals[0], resolveOf: (id) => resolve.get(id), reopenOf: (id) => reopen.get(id) };
  }, [refusals, idsKey]);
}
