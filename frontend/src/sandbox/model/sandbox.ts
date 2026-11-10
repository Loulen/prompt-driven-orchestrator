/**
 * Hand-written shapes of the staging-profile payloads, built with `json!` on the
 * daemon side (no Rust DTO yet).
 */

import type { SandboxEntryKind } from "../../shared/generated/SandboxEntryKind";
import type { SandboxProfileImage } from "../../shared/generated/SandboxProfileImage";

/** One staging profile, as `GET /settings` lists it (#432): the NAME only. */
export interface SandboxProfileRef {
  name: string;
  /** `full` / `minimal` — resolves with no DB row until edited (ADR-0031 §2). */
  virtual: boolean;
}

/** One resolved entry of a staging profile, as the editor shows it (#432). */
export interface SandboxProfileEntry {
  /** `$HOME`-relative path, or the default's one-level glob pattern. */
  path: string;
  kind: SandboxEntryKind;
  /** From the built-in default (checkable) vs a user extra (removable). */
  from_default: boolean;
  enabled: boolean;
  /**
   * Class (b): unchecking does NOT make the file absent — the staging set's fixup
   * re-synthesises the keys it needs. Exactly two entries. The UI must say so, or
   * unchecking reads as more destructive than it is.
   */
  resynthesised: boolean;
  /** Server-owned advisory (disk cost, behaviour when unchecked). Never derived client-side. */
  note: string | null;
  /** Under `.ssh` / `.aws` / `.gnupg` — allowed with a warning (ADR-0031 §3). */
  sensitive: boolean;
  /** Present on the host right now; `null` when unknowable (a glob, or no `$HOME`). */
  exists: boolean | null;
}

/**
 * One class-(c) staging-set guarantee (#432, ADR-0063): satisfied by the WHOLE file, so it
 * is neither checkable nor addable. Rendered read-only — without this block a `minimal`
 * profile's screen looks broken and the user wrongly concludes the container starts with no
 * credentials. Served under the historical wire key `floor`.
 */
export interface SandboxStagingGuarantee {
  id: string;
  label: string;
  path: string | null;
}

/** The full editor view of one staging profile (`GET|PUT /settings/sandbox-profiles/{name}`). */
export interface SandboxProfile {
  name: string;
  virtual: boolean;
  /** A DB row exists (the profile has been edited at least once). */
  materialised: boolean;
  /** The stored DIFF — the user's intention, never the effective list. */
  disabled: string[];
  extras: string[];
  /** What a Run created NOW would freeze into `RunStarted`. */
  resolved: string[];
  entries: SandboxProfileEntry[];
  /** Signalled no-ops, never errors (ADR-0031 §2). */
  redundant_extras: string[];
  inactive_disabled: string[];
  floor: SandboxStagingGuarantee[];
  sensitive_prefixes: string[];
  /**
   * Environment variables posed at `docker create` for every Run on this profile (#468,
   * ADR-0031 §8). **Not a diff** — unlike `disabled`/`extras` there is no built-in default
   * to fold against, so this map IS the effective env.
   *
   * Values are served in clear, on purpose: they already sit in clear in SQLite, in the
   * Run's frozen `run_started` payload and in `docker inspect`. The sandbox is not a
   * security boundary and the editor says so — masking them here would suggest PDO is
   * protecting something it is not.
   */
  env: Record<string, string>;
  /**
   * The keys PDO poses itself and therefore refuses with a 400 naming the key (`HOME`,
   * `PDO_DAEMON_URL`, `PDO_RUN_ID`). Server-owned so the editor never hard-codes a parallel
   * list that would drift the day a fourth run-constant appears (#373).
   */
  reserved_env_keys: string[];
  /**
   * Where this profile's container image comes from (#467, ADR-0031 §9), or `null` when it poses
   * nothing and PDO's built-in default decides (#471: registry-pulled, hash-derived from the
   * seeded Dockerfile). Like `env` it is a FULL replacement on write — `null` is how you go back
   * to the default image.
   */
  image: SandboxProfileImage | null;
  updated_at: string | null;
}

/**
 * The body of `PUT /settings/sandbox-profiles/{name}`: the profile's **diff** —
 * `disabled` / `extras`, never a snapshot (ADR-0031 §2) — plus its `env` map and its
 * `image` source, neither of which is a diff at all. Every field is a FULL replacement.
 */
export interface SandboxProfileDiff {
  disabled: string[];
  extras: string[];
  env: Record<string, string>;
  image: SandboxProfileImage | null;
}

/**
 * Who still points at a profile (`GET …/{name}/referents`, #432). Server-side because the
 * frontend cannot derive the third class: `RunListEntry` carries no `sandbox`.
 *
 * The distinction is the whole point of the delete dialog: `instance_default` and
 * `triggers` are NOT repointed and their next Run **fails**, while `runs` already froze
 * their entry list at start and are **unaffected**.
 */
export interface SandboxProfileReferents {
  name: string;
  instance_default: boolean;
  triggers: { id: string; name: string; enabled: boolean }[];
  runs: { run_id: string; pipeline_name: string | null; name: string | null }[];
}
