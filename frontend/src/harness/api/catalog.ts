// The harness catalog is served inside `GET /settings` (`harness_descriptors`,
// #553, #586): the harness context reads it from there.

import { fetchSettings } from "../../api";
import { harnessCatalog, type HarnessCatalog } from "../model/catalog";

/** The picker catalog, from a fresh `GET /settings`. */
export async function fetchHarnessCatalog(): Promise<HarnessCatalog> {
  const settings = await fetchSettings();
  return harnessCatalog(settings.harness_descriptors);
}
