/**
 * The frames of `/ws`, the live stream the run context serves: the daemon's
 * event bus relayed as is (a Run event, or an announcement of another
 * context), plus the `ready` greeting and the heartbeat. Hand-written: the
 * frames are built with `json!` on the daemon side.
 */

import type { DaemonEvent } from "../../shared/generated/DaemonEvent";

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
