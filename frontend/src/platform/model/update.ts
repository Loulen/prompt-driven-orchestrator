/**
 * Hand-written shapes of the version-check payloads (#697, #699), built with
 * `json!` on the daemon side (`crates/pdo-platform/src/adapters/http/update.rs`).
 */

import type { InstallMethod } from "../../shared/generated/InstallMethod";
import type { Supervision } from "../../shared/generated/Supervision";
import type { UpdateAttempt } from "../../shared/generated/UpdateAttempt";
import type { UpdateAttemptStatus } from "../../shared/generated/UpdateAttemptStatus";

/**
 * `GET /update` (#697): the daemon's cached version-check state. Every field is read
 * from the cache — a page load never triggers a request to the release source.
 * `latest_version` is `null` when unknown, unreachable at last check, or the check is
 * off, or no release source is declared; `reason` then says which. Never a false
 * "up to date".
 */
export interface UpdateStatus {
  /**
   * The release channel's name (#24): `PDO` for the vanilla binary, the
   * distribution's name (`acme-pdo`…) otherwise.
   */
  channel_name: string;
  /** The version the channel declares, compared with its releases. */
  installed_version: string;
  /** The version of the core the binary is built on (equal to `installed_version` for `PDO`). */
  core_version: string;
  latest_version: string | null;
  /** `true` exactly when `latest_version` is strictly newer than the installed one. */
  newer_available: boolean;
  /** RFC3339 date of the last check, success or failure; `null` if never. */
  checked_at: string | null;
  /** Human label of the source (`GitHub Releases`, or the override's host); `null` without a source. */
  source: string | null;
  /** `null` when the channel declares no release source: nothing is checked. */
  source_url: string | null;
  /** The page a human opens for the source (GitHub: the repository's releases page), for the tooltip; `null` without a source. */
  source_page_url: string | null;
  check_enabled: boolean;
  install_method: InstallMethod;
  /** The exact command a manual update runs; `null` when the channel declares none. */
  manual_command: string | null;
  supervision: Supervision;
  /** Why `latest_version` is `null`, when it is. */
  reason: string | null;
  /** The last check's error, kept beside the last good values. */
  last_error: string | null;
  /** Live Runs (running / awaiting user / paused) — the confirm dialog's count (#699). Informational: never a block. */
  active_runs: number;
  /** `POST /update/apply` would be accepted: a known install method or a declared command, and no attempt running. */
  can_apply: boolean;
  /** Why `can_apply` is false, when it is (unknown method, no declared command, attempt running). */
  apply_blocked_reason: string | null;
  /** The last update attempt journaled under `~/.pdo/update/`, including a failed one. */
  last_attempt: UpdateAttempt | null;
}

/** `POST /update/apply` — 202 with the attempt just spawned (#699). */
export interface UpdateApplyResponse {
  attempt_id: string;
  status: UpdateAttemptStatus;
  method: InstallMethod;
  command: string;
  supervision: Supervision;
  log_path: string;
  started_at: string;
  from_version: string;
}
