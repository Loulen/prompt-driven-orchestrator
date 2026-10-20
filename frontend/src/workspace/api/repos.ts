/**
 * Repositories over HTTP: validate one, list and refresh its branches,
 * fast-forward one (#802, #803), read its remote, create one (#824), clone one
 * by its URL (CORE-97), and the recent ones.
 */

import { apiErrorMessage, request } from "../../shared/http/client";
import type { BranchList } from "../../shared/generated/BranchList";
import type { CloneJob } from "../../shared/generated/CloneJob";
import type { CloneTarget } from "../../shared/generated/CloneTarget";
import type { CreateRepoFile } from "../../shared/generated/CreateRepoFile";
import type { FastForwardRefusal } from "../../shared/generated/FastForwardRefusal";
import type { FastForwardResult } from "../../shared/generated/FastForwardResult";
import type { RepoRemote } from "../../shared/generated/RepoRemote";
import type { FastForwardOutcome } from "../model/branches";

export interface ValidateRepoResponse {
  valid: boolean;
  error?: string;
}

export async function validateRepo(path: string): Promise<ValidateRepoResponse> {
  // No `resp.ok` check by contract: the `{ valid, error }` body is authoritative
  // even on a non-2xx, so raw mode reads the body unconditionally (never throws).
  const resp = await request<Response>(
    "GET",
    `/repos/validate?path=${encodeURIComponent(path)}`,
    { responseMode: "raw" },
  );
  return resp.json();
}

export interface CreateRepoResponse {
  /** Absolute path of the repository, created or already there. */
  path: string;
  /** `false` when an existing repository was reused untouched. */
  created: boolean;
}

/**
 * Create a git repository at `<parent>/<name>` with `files` and one commit on
 * `main` (#824) — a generic verb, not a tour one. Idempotent: an existing
 * repository at that path is handed back as is, with no second commit and no
 * write over what the user did in it. An existing empty folder becomes the
 * repository; any other existing *non*-repository is refused, and the
 * {@link ApiError} carries the daemon's sentence for quoting verbatim.
 *
 * The created path is deliberately NOT added to recent repositories — that list
 * is built from Runs, and nothing has run here yet.
 */
export function createRepo(
  parent: string,
  name: string,
  files: CreateRepoFile[] = [],
): Promise<CreateRepoResponse> {
  return request<CreateRepoResponse>("POST", "/repos/create", {
    body: { parent, name, files },
    label: "POST /repos/create",
  });
}

/** The remote of the repository at `path`, credentials dropped; `null` without one. */
export function fetchRepoRemote(path: string): Promise<RepoRemote> {
  return request<RepoRemote>("GET", `/repos/remote?path=${encodeURIComponent(path)}`, {
    label: "GET /repos/remote",
  });
}

export function listBranches(repoPath: string): Promise<BranchList> {
  return request<BranchList>(
    "GET",
    `/repos/branches?path=${encodeURIComponent(repoPath)}`,
    { label: "GET /repos/branches" },
  );
}

/**
 * Fetch every remote of `repoPath` (pruning refs that vanished), then answer the
 * refreshed list (#802, ADR-0070 §1).
 *
 * Read-only on the remote side and never blocking on this side: a fetch that fails
 * still resolves, with `fetch_error` set and the on-disk list intact. Callers must
 * NOT treat a failure as a reason to clear branches or to refuse a launch — a repo
 * that is offline, has no remote, or has no key is a legitimate repo.
 *
 * Only one fetch per repo runs at a time daemon-side: overlapping asks (form opened,
 * repo changed, sync clicked) join the one in flight.
 */
export function fetchRemotes(repoPath: string): Promise<BranchList> {
  return request<BranchList>(
    "POST",
    `/repos/fetch?path=${encodeURIComponent(repoPath)}`,
    { label: "POST /repos/fetch" },
  );
}

/**
 * Fast-forward a local branch onto its tracking branch (#803, ADR-0070 §2).
 *
 * Raw mode: a 409 here is a **named refusal**, not a failed call — it carries the
 * reason to show AND the refreshed list, so letting `request` throw would lose both.
 * The only write PDO performs on a local branch; never a merge, rebase or push.
 *
 * Callers must treat every outcome as non-blocking: a refusal changes nothing in
 * the repo and never stops a launch (the shortcut to `origin/<branch>` is the exit
 * that always works).
 */
export async function fastForwardBranch(
  repoPath: string,
  branch: string,
): Promise<FastForwardOutcome> {
  let resp: Response;
  try {
    resp = await request<Response>(
      "POST",
      `/repos/fast-forward?path=${encodeURIComponent(repoPath)}`,
      { body: { branch }, responseMode: "raw", label: "POST /repos/fast-forward" },
    );
  } catch (e) {
    return { kind: "error", message: e instanceof Error ? e.message : String(e) };
  }
  const parsed: unknown = await resp.json().catch(() => null);
  if (resp.ok) {
    const b = parsed as { fast_forward: FastForwardResult } & BranchList;
    return { kind: "done", result: b.fast_forward, list: listOf(b) };
  }
  const b = parsed as ({ refusal?: FastForwardRefusal } & Partial<BranchList>) | null;
  if (b?.refusal) {
    // The list travels with the refusal: two of the five reasons are races the
    // client heals by simply re-reading the truth.
    return {
      kind: "refused",
      refusal: b.refusal,
      list: Array.isArray(b.branches) ? listOf(b as BranchList) : null,
    };
  }
  return {
    kind: "error",
    message: apiErrorMessage(parsed, `fast-forward refused: ${resp.status}`),
  };
}

/** Lift the flattened `BranchList` out of a fast-forward response body. */
function listOf(b: BranchList): BranchList {
  return {
    branches: b.branches,
    last_fetch_at: b.last_fetch_at ?? null,
    fetch_error: b.fetch_error ?? null,
  };
}

export function fetchRecentRepos(): Promise<string[]> {
  return request<string[]>("GET", "/repos/recent");
}

/**
 * Where the clone of `url` goes (CORE-97): `<clones folder>/<name>`, and whether
 * that folder already is a clone of it (reused, not cloned again).
 */
export function fetchCloneTarget(url: string): Promise<CloneTarget> {
  return request<CloneTarget>("GET", "/repos/clone-target", {
    query: { url },
    label: "GET /repos/clone-target",
  });
}

/**
 * Clone `url` in full, in the background (CORE-97). The answer is the clone as it
 * starts, or done at once when the URL is already cloned there (`reused`). With a
 * Projet, the repository joins it once cloned — even if the page is closed by then.
 */
export function startClone(body: {
  url: string;
  /** The folder to clone into; absent → `<clones folder>/<name>`. */
  path?: string;
  project_id?: string;
}): Promise<CloneJob> {
  return request<CloneJob>("POST", "/repos/clones", { body, label: "POST /repos/clones" });
}

/** The clones running, and the last ones finished, newest first. */
export function fetchClones(): Promise<CloneJob[]> {
  return request<CloneJob[]>("GET", "/repos/clones", { label: "GET /repos/clones" });
}

export function fetchClone(id: string): Promise<CloneJob> {
  return request<CloneJob>("GET", `/repos/clones/${encodeURIComponent(id)}`, {
    label: "GET /repos/clones",
  });
}

/** Stop a running clone: nothing it wrote stays. */
export function cancelClone(id: string): Promise<CloneJob> {
  return request<CloneJob>("DELETE", `/repos/clones/${encodeURIComponent(id)}`, {
    label: "DELETE /repos/clones",
  });
}
