/**
 * The row metadata every per-pipeline Stats table shares (a client-side view of
 * fields the daemon repeats inline on each entity).
 */

import type { StatsAbsorbedMember } from "../../shared/generated/StatsAbsorbedMember";

/**
 * What a Pipeline row carries beside its numbers (#890): the Runs it counts,
 * the most recent one's start (ISO), and — on an **absorbent** — its absorbed
 * members (ADR-0077). Node rows and « By model » model rows carry the same
 * (#892); the daemon omits it on every other level.
 */
export interface StatsPipelineRowMeta {
  runs?: number;
  last_run?: string | null;
  absorbed?: StatsAbsorbedMember[];
}
