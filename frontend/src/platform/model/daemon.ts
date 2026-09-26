/**
 * Hand-written shapes of the daemon-level payloads that have no Rust DTO yet
 * (built with `json!` on the daemon side): session counter, version check, `/ws` frames.
 */

import type { DaemonEvent } from "../../shared/generated/DaemonEvent";
import type { InstallMethod } from "../../shared/generated/InstallMethod";
import type { ServiceHealth } from "../../shared/generated/ServiceHealth";
import type { Supervision } from "../../shared/generated/Supervision";
import type { UpdateAttempt } from "../../shared/generated/UpdateAttempt";
import type { UpdateAttemptStatus } from "../../shared/generated/UpdateAttemptStatus";

/**
 * Live NodeRun-session count, the configured global cap, the daemon version,
 * and the persistent-service health, for the bottom status bar (#159 /
 * ADR-0012, #139, #156). Manager sessions are excluded. `version` and
 * `service` are absent until the daemon has responded.
 */
export interface DaemonStatus {
  live: number;
  cap: number;
  version?: string;
  service?: ServiceHealth;
}

/**
 * `GET /update` (#697): the daemon's cached version-check state. Every field is read
 * from the cache — a page load never triggers a request to the release source.
 * `latest_version` is `null` when unknown, unreachable at last check, or the check is
 * off; `reason` then says which. Never a false "up to date".
 */
export interface UpdateStatus {
  installed_version: string;
  latest_version: string | null;
  /** `true` exactly when `latest_version` is strictly newer than the installed one. */
  newer_available: boolean;
  /** RFC3339 date of the last check, success or failure; `null` if never. */
  checked_at: string | null;
  /** Human label of the source (`GitHub Releases`, or the override's host). */
  source: string;
  source_url: string;
  check_enabled: boolean;
  install_method: InstallMethod;
  /** The exact command a manual update runs for this install method. */
  manual_command: string;
  supervision: Supervision;
  /** Why `latest_version` is `null`, when it is. */
  reason: string | null;
  /** The last check's error, kept beside the last good values. */
  last_error: string | null;
  /** Live Runs (running / awaiting user / paused) — the confirm dialog's count (#699). Informational: never a block. */
  active_runs: number;
  /** `POST /update/apply` would be accepted: a known install method and no attempt running. */
  can_apply: boolean;
  /** Why `can_apply` is false, when it is (unknown method, attempt running). */
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

export interface WsMessage {
  type:
    | "ready"
    | "heartbeat"
    | "event"
    | "pipeline_changed"
    | "trigger_created"
    | "trigger_fired"
    | "trigger_updated"
    | "trigger_deleted"
    | "triggers_paused"
    | "project_changed"
    | "cost_prices_synced"
    | "run_forgotten";
  event?: DaemonEvent;
  pipeline_id?: string;
  path?: string;
  ts?: string;
  /** Set on trigger_* messages (#160). */
  trigger_id?: string;
  outcome?: string;
  run_id?: string | null;
  /** Set on `triggers_paused` messages (#348): the new global pause state. */
  paused?: boolean;
  /** Set on `project_changed` messages (#552): the mutated Projet's id. */
  project_id?: string;
  /** Set on `cost_prices_synced` messages: how many price rows the sync wrote. */
  rows?: number;
}
