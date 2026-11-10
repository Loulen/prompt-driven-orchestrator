/**
 * The Operations behind the gestures of the Stats absorptions (CORE-205), as
 * the question of authorization asks them to grey a refused one.
 */
import type { OperationQuestion } from "../../shared/authorization/api";

/** The Operation behind the ✕ of `member`: `stats_absorption` × `delete` on its key. */
export function uncombineQuestion(member: { key: string }): OperationQuestion {
  return { resource: "stats_absorption", verb: "delete", target: member.key };
}

/** The Operation behind Combine: `stats_absorption` × `create`. */
export const COMBINE_QUESTION: OperationQuestion = { resource: "stats_absorption", verb: "create" };
