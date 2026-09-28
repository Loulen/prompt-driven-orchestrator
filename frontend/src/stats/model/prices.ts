/**
 * What a price sync reports back to the Stats modal.
 */

/**
 * Response of `POST /settings/cost-prices/sync` (#427, ADR-0034).
 *
 * A noop is an honest 200 carrying `noop: true` + `reason` (ADR-0025 forbids a
 * blind `{ok:true}`); a network cut is a thrown 502 naming the source.
 */
export interface SyncCostPricesReport {
  ok: boolean;
  noop?: boolean;
  reason?: string | null;
  source: string;
  fetched_at: string | null;
  /** Rows retained in the fetched tier. */
  rows: number;
  /** Keys no tier priced before — the repair. */
  added: string[];
  /** Keys whose effective price changes. */
  updated: string[];
  unchanged: number;
  /** Source rows refused, with the motive. */
  rejected: string[];
  /** Fetched, but the manual tier still wins — said, never hidden. */
  shadowed_by_manual: string[];
}
