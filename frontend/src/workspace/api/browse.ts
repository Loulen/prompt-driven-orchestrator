/**
 * The filesystem explorer over HTTP (`GET /fs/browse`): one level of a
 * directory, read only.
 */

import { request } from "../../shared/http/client";
import type { BrowseEntry } from "../../shared/generated/BrowseEntry";

export interface BrowseResponse {
  /** The directory actually listed (canonicalized). */
  path: string;
  /** Parent directory, or null only at the filesystem root. */
  parent: string | null;
  entries: BrowseEntry[];
  /** True iff the post-filter entry count exceeded the listing cap. */
  truncated: boolean;
  /** Non-null when the dir was navigable but unlistable (e.g. permission denied). */
  error: string | null;
}

/**
 * Optional widening of the listing (#431). Both flags are **off** by default, which
 * is the pre-rename behaviour bit for bit: directories only, dot-entries filtered.
 * A flag only travels on the wire when `true`, so the default call's URL is exactly
 * `/fs/browse`, with no query string at all.
 */
export interface BrowseOptions {
  files?: boolean;
  hidden?: boolean;
}

/**
 * List `path` (or the daemon's default chain `$HOME → repo_root → /` when omitted).
 * 200 always carries the {@link BrowseResponse} shape — including the in-body
 * `error` for navigable-but-unlistable dirs — so callers branch on `data.error`.
 * Only genuine caller/system bugs (relative path → 400, collapsed default → 500)
 * throw here.
 *
 * `path` stays the FIRST positional parameter, and in default mode callers must pass
 * it as the SOLE argument: `RepoCombobox.test.tsx` pins `browseFs(undefined)` /
 * `browseFs("/abs/repo/path")` and vitest compares arity strictly (a trailing
 * `undefined` is a recorded second argument and breaks `toHaveBeenCalledWith`).
 */
export function browseFs(path?: string, opts: BrowseOptions = {}): Promise<BrowseResponse> {
  return request<BrowseResponse>("GET", "/fs/browse", {
    // `request` drops undefined-valued keys, so `|| undefined` keeps `files=false`
    // off the wire instead of sending a redundant explicit default.
    query: { path, files: opts.files || undefined, hidden: opts.hidden || undefined },
    label: "GET /fs/browse",
  });
}
