// The harness catalog is served inside `GET /settings` (`harness_descriptors`,
// #553, #586): the harness context reads it from there.

import { fetchSettings } from "../../settings/api/settings";
import { request } from "../../shared/http/client";
import { harnessCatalog, type HarnessCatalog } from "../model/catalog";
import type { HarnessListItem } from "../model/harness";

/** The picker catalog, from a fresh `GET /settings`. */
export async function fetchHarnessCatalog(): Promise<HarnessCatalog> {
  const settings = await fetchSettings();
  return harnessCatalog(settings.harness_descriptors);
}

/** Probe the model catalogue of `harness` again now, even at the same binary
 *  version: its entry of `harness_descriptors.harnesses`, as re-read. Every
 *  open page hears of it (`harness_catalogue_changed`). */
export function refreshHarnessModels(harness: string): Promise<HarnessListItem> {
  return request<HarnessListItem>("POST", `/settings/harnesses/${encodeURIComponent(harness)}/refresh`, {
    label: "Refresh models",
  });
}
