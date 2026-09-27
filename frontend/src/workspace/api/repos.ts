/**
 * Repositories over HTTP: validate one, list and refresh its branches,
 * fast-forward one (#802, #803), create one (#824), and the recent ones.
 */

import { apiErrorMessage, request } from "../../shared/http/client";
import type { BranchList } from "../../shared/generated/BranchList";
import type { CreateRepoFile } from "../../shared/generated/CreateRepoFile";
import type { FastForwardRefusal } from "../../shared/generated/FastForwardRefusal";
import type { FastForwardResult } from "../../shared/generated/FastForwardResult";
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
 * write over what the user did in it. An existing *non*-repository is refused,
 * and the {@link ApiError} carries the daemon's sentence for quoting verbatim.
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
