import type { InstanceSettings, SyncCostPricesReport, AgentProfileReferents } from "./settings/model/settings";
import type { UpdateSettingsRequest } from "./shared/generated/UpdateSettingsRequest";
import type { StatsOverview } from "./shared/generated/StatsOverview";
import type { StatsCost } from "./shared/generated/StatsCost";
import type { StatsPerformance } from "./shared/generated/StatsPerformance";
import type { StatsAbsorption } from "./shared/generated/StatsAbsorption";
import type { StatsAbsorptionList } from "./shared/generated/StatsAbsorptionList";
import type { AgentProfile } from "./shared/generated/AgentProfile";
import type { AgentProfileInput } from "./shared/generated/AgentProfileInput";
import { request } from "./shared/http/client";

export function fetchSettings(): Promise<InstanceSettings> {
  return request<InstanceSettings>("GET", "/settings");
}

/**
 * Persist one or more instance-config knobs and return the recomputed view
 * (#129, ADR-0015). Surfaces the daemon's fail-fast validation error (`400`)
 * verbatim so the modal can show it.
 */
export function updateSettings(
  patch: UpdateSettingsRequest,
): Promise<InstanceSettings> {
  return request<InstanceSettings>("PUT", "/settings", { body: patch });
}

export function fetchAgentProfiles(): Promise<{ profiles: AgentProfile[] }> {
  return request("GET", "/settings/agent-profiles");
}

export function createAgentProfile(
  profile: AgentProfileInput,
): Promise<AgentProfile> {
  return request("POST", "/settings/agent-profiles", { body: profile });
}

export function updateAgentProfile(
  id: string,
  profile: AgentProfileInput,
): Promise<AgentProfile> {
  return request("PUT", `/settings/agent-profiles/${encodeURIComponent(id)}`, {
    body: profile,
  });
}

export function deleteAgentProfile(id: string): Promise<void> {
  return request("DELETE", `/settings/agent-profiles/${encodeURIComponent(id)}`, {
    responseMode: "void",
  });
}

export function fetchAgentProfileReferents(id: string): Promise<AgentProfileReferents> {
  return request(
    "GET",
    `/settings/agent-profiles/${encodeURIComponent(id)}/referents`,
  );
}

//
/**
 * Fetch the remote price source and rewrite the fetched price tier (#427,
 * ADR-0034). The daemon's only outbound call from a user gesture.
 *
 * Under `/settings/…` deliberately: the vite dev proxy keys on a PREFIX, so this
 * needs no `vite.config.ts` line — the trap `/nodes` (#345), `/stats` (#377) and
 * `/fs` (#431) each paid, where a missing proxy entry makes a dev-mode request
 * answer 200 with the SPA.
 *
 * Throws on 409 (a sync is already in flight) and on 502 (source unreachable, or
 * an empty harvest — in which case nothing was written and the last known table
 * survives). A run with nothing to change resolves with `noop: true`.
 */
export function syncCostPrices(): Promise<SyncCostPricesReport> {
  return request<SyncCostPricesReport>("POST", "/settings/cost-prices/sync", {
    label: "POST /settings/cost-prices/sync",
  });
}

/**
 * Cheap instance stats over `[from, to)` bucketed by `bucket` (#377): runs,
 * errors (`run_failed`), sessions, fires-per-pipeline, and the "triggers that
 * created a run" KPI. Indexed SQL — safe to fetch on modal open.
 * `completed_only` (#810) narrows the Run cohort to Runs that reached
 * `completed`; the Errors series then reads zero rather than disappearing.
 */
export function fetchStatsOverview(
  from: string,
  to: string,
  bucket: string,
  completedOnly = false,
  uncombined = false,
): Promise<StatsOverview> {
  return request<StatsOverview>("GET", "/stats/overview", {
    query: {
      from,
      to,
      bucket,
      ...(completedOnly ? { completed_only: true } : {}),
      // #891 — « Uncombined »: the rows as the event log wrote them. Omitted
      // when off, so an unchanged call sends the query it always did.
      ...(uncombined ? { uncombined: true } : {}),
    },
  });
}

/**
 * Estimated cost over `[from, to)`, folded by period/pipeline/project (#377,
 * ADR-0022/0029). Heavy (memoized per-run cost fanned over the window) — fetch
 * lazily, only when the cost tab is shown.
 */
export function fetchStatsCost(
  from: string,
  to: string,
  bucket: string,
  completedOnly = false,
  uncombined = false,
): Promise<StatsCost> {
  return request<StatsCost>("GET", "/stats/cost", {
    query: {
      from,
      to,
      bucket,
      ...(completedOnly ? { completed_only: true } : {}),
      ...(uncombined ? { uncombined: true } : {}),
    },
  });
}

/**
 * Context, wall-clock and **active** duration distributions. Heavy journal
 * reads, so callers load it lazily. `completed_only` (#810) narrows the cohort
 * to Runs that reached `completed`; omitted when false, so an unchanged call
 * sends the byte-identical query it always did.
 */
export function fetchStatsPerformance(
  from: string,
  to: string,
  refresh = false,
  completedOnly = false,
  uncombined = false,
): Promise<StatsPerformance> {
  return request<StatsPerformance>("GET", "/stats/performance", {
    query: {
      from,
      to,
      ...(refresh ? { refresh: true } : {}),
      ...(completedOnly ? { completed_only: true } : {}),
      ...(uncombined ? { uncombined: true } : {}),
    },
  });
}

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

