/**
 * The staging-profile routes (`/settings/sandbox-profiles*`), typed. The sandbox
 * context's slice of the HTTP client.
 */

import { request } from "../../shared/http/client";
import type {
  SandboxProfile,
  SandboxProfileDiff,
  SandboxProfileReferents,
} from "../model/sandbox";

// A separate REST resource, NOT part of the grouped `PUT /settings`: a profile is a ROW,
// not a `{effective, source, stored, env, default}` knob. That is exactly why the editor's
// footer says "Done" rather than "Save" — nothing is batched behind it.
//
// The routes sit under `/settings/…` purely for ROUTING: the vite proxy key is a prefix,
// so `'/settings'` already covers every sub-path (no proxy edit, none of the "dev GET
// answers 200 with the SPA" traps that `/nodes`, `/stats` and `/fs` each paid).

/** Every staging profile, each fully resolved (`+ home`, the host `$HOME`). */
export function fetchSandboxProfiles(): Promise<{
  profiles: SandboxProfile[];
  home: string | null;
}> {
  return request("GET", "/settings/sandbox-profiles");
}

/** One resolved profile; throws `ApiError` with status 404 when the name is unknown. */
export function fetchSandboxProfile(name: string): Promise<SandboxProfile> {
  return request<SandboxProfile>(
    "GET",
    `/settings/sandbox-profiles/${encodeURIComponent(name)}`,
  );
}

/**
 * Upsert a profile's **diff** — `disabled` / `extras`, never a snapshot (ADR-0031 §2) —
 * plus its `env` map (#468, ADR-0031 §8) and its `image` source (#467, ADR-0031 §9), neither
 * of which is a diff at all.
 * Upsert, because the caller cannot know whether `full` already has a row, and editing it
 * IS what materialises one. Returns the recomputed view so the editor needs no refetch.
 *
 * Every field is a FULL replacement, `env` and `image` included: omitting a variable is how you
 * remove it, and `image: null` is how you go back to the instance-wide setting. So every caller
 * passes the fields it is NOT changing verbatim — which is why the editor threads them through one
 * `write` helper rather than per-control.
 */
export function saveSandboxProfile(
  name: string,
  diff: SandboxProfileDiff,
): Promise<SandboxProfile> {
  return request<SandboxProfile>(
    "PUT",
    `/settings/sandbox-profiles/${encodeURIComponent(name)}`,
    { body: diff },
  );
}

/**
 * Delete the materialised row. Unconditional (ADR-0031 §7 — a *soft* guard-rail, no
 * referential integrity in the DB): deleting an edited `full`/`minimal` reverts it to its
 * virtual default, deleting a user profile makes its referents' next Run fail loud.
 * Neither repoints anything, which is what {@link fetchSandboxProfileReferents} is for.
 */
export function deleteSandboxProfile(name: string): Promise<void> {
  return request<void>(
    "DELETE",
    `/settings/sandbox-profiles/${encodeURIComponent(name)}`,
    { responseMode: "void" },
  );
}

/** Who still points at a profile — server-side, because `RunListEntry` carries no `sandbox`. */
export function fetchSandboxProfileReferents(
  name: string,
): Promise<SandboxProfileReferents> {
  return request<SandboxProfileReferents>(
    "GET",
    `/settings/sandbox-profiles/${encodeURIComponent(name)}/referents`,
  );
}
