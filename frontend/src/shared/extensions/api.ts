/**
 * The extensions as the daemon declares them (`GET /ext`): each one's id, name,
 * version, UI bundle and the UI slots it fills. `[]` without extension.
 */

import type { ExtensionEntry } from "../generated/ExtensionEntry";
import { request } from "../http/client";

export function fetchExtensions(): Promise<ExtensionEntry[]> {
  return request<ExtensionEntry[]>("GET", "/ext");
}
