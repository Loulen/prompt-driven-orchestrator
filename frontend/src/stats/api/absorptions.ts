/**
 * The Stats absorptions (ADR-0077): list, combine, uncombine.
 */

import type { StatsAbsorption } from "../../shared/generated/StatsAbsorption";
import type { StatsAbsorptionList } from "../../shared/generated/StatsAbsorptionList";
import { request } from "../../shared/http/client";

/** Every Stats absorption of the instance (#890, ADR-0077). */
export function fetchStatsAbsorptions(): Promise<StatsAbsorptionList> {
  return request<StatsAbsorptionList>("GET", "/stats/absorptions");
}

/**
 * Combine Stats rows (#890, #892): `absorbent` keeps its name and counts
 * `members`' data in every tab that has the dimension. Flattened by the daemon —
 * a member that was itself an absorbent hands its members over. A Node
 * absorption names the Pipeline row its Nodes sit under (`scope`), and each
 * Node its own: the daemon refuses Nodes of two Pipelines. Resolves to the list
 * as it stands after.
 */
export function combineStatsRows(body: {
  dimension: StatsAbsorption["dimension"];
  scope?: string;
  scope_name?: string;
  absorbent: { key: string; name: string; scope?: string };
  members: { key: string; name: string; scope?: string }[];
}): Promise<StatsAbsorptionList> {
  return request<StatsAbsorptionList>("POST", "/stats/absorptions", { body });
}

/** Take one member of any dimension out of its absorption — the ✕ of the
 *  members list in Stats and of Settings › General › Stats absorptions (#891).
 *  `scope` is empty outside a Node absorption. */
export function uncombineStatsMember(
  dimension: StatsAbsorption["dimension"],
  scope: string,
  member: string,
): Promise<StatsAbsorptionList> {
  return request<StatsAbsorptionList>("DELETE", "/stats/absorptions/member", {
    query: { dimension, ...(scope ? { scope } : {}), member },
  });
}
