/**
 * The price table the Cost tab reads, refreshed from the Stats modal.
 */

import type { SyncCostPricesReport } from "../model/prices";
import { request } from "../../shared/http/client";

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
