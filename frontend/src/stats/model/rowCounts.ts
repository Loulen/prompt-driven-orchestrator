/**
 * How each tab counts a row it may combine (#890): the figure the members list
 * and the combine bar name beside each row.
 */

import type { StatsAbsorbedMember } from "../../shared/generated/StatsAbsorbedMember";
import type { StatsSessionEntity } from "../../shared/generated/StatsSessionEntity";
import type { AbsorbableRow } from "./statsAbsorption";

/** Sessions counts a Pipeline in executions, like its column. */
export const SESSIONS_COUNT = {
  of: (row: StatsSessionEntity) => row.executions,
  ofMember: (member: StatsAbsorbedMember) => member.executions ?? member.runs,
  word: "execution" as const,
};

/** Cost and Performance count a Pipeline, a Node or a Model in Runs. */
export const RUNS_COUNT = {
  of: (row: AbsorbableRow) => row.runs ?? 0,
  ofMember: (member: StatsAbsorbedMember) => member.runs,
  word: "run" as const,
};
