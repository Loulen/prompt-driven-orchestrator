/**
 * Hand-written shapes of the daemon-level payloads that have no Rust DTO yet
 * (built with `json!` on the daemon side): session counter, `/ws` frames. The
 * version check's live in `./update`.
 */

import type { DaemonEvent } from "../../shared/generated/DaemonEvent";
import type { ServiceHealth } from "../../shared/generated/ServiceHealth";

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
