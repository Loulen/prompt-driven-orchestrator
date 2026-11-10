/**
 * The frames of `/ws`, the live stream the run context serves: the daemon's
 * event bus relayed as is (a Run event, or an announcement of another
 * context), plus the `ready` greeting, the heartbeat and `resync`. Hand-written: the
 * frames are built with `json!` on the daemon side.
 */

import type { DaemonEvent } from "../../shared/generated/DaemonEvent";
import type { Actor } from "../../shared/generated/Actor";
import type { Presence } from "../../shared/generated/Presence";
import type { DraftSummary } from "../../shared/generated/DraftSummary";
import type { HostedLiveArtifact, HostedSession } from "../../session/model/hostedSession";

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
    /** A harness's model catalogue was probed again (« Refresh models »). */
    | "harness_catalogue_changed"
    /** The instance settings were written (UI, CLI or another browser). */
    | "instance_settings_changed"
    | "pipeline_validation_changed"
    /** Who has a Pipeline's shared draft open, and its changes (ADR-0088). */
    | "pipeline_draft_changed"
    /** A Save numbered a new version of a Pipeline (ADR-0088). */
    | "pipeline_version_changed"
    | "cost_prices_synced"
    | "run_forgotten"
    /** An Actor opened a Run, or no longer has it open anywhere (API 0.12). */
    | "presence_arrived"
    | "presence_left"
    /** A Session hébergée changed (API 0.19): its state, its Zoom du terminal. */
    | "hosted_session_changed"
    /** Its extension forgot a Session hébergée (API 0.19). */
    | "hosted_session_forgotten"
    /** The Artefacts live of a Session hébergée changed (API 0.19). */
    | "hosted_live_artifacts_changed"
    /** An event an extension published (`Host::publish`). */
    | "extension_event"
    /** Re-read the whole state (ADR-0083 §6): sent by the daemon to a client
     *  that lost messages, and emitted by the stream itself after each
     *  (re)opening, a return to the tab and a return of the network. */
    | "resync";
  event?: DaemonEvent;
  pipeline_id?: string;
  /** Set on `pipeline_draft_changed`. */
  draft?: DraftSummary;
  path?: string;
  ts?: string;
  /** Set on trigger_* messages (#160); a `trigger_updated` without it is
   *  about every Trigger (an extension signalled their suspensions). */
  trigger_id?: string;
  outcome?: string;
  run_id?: string | null;
  /** Set on `triggers_paused` messages (#348): the new global pause state. */
  paused?: boolean;
  /** Set on `project_changed` messages (#552): the mutated Projet's id. */
  project_id?: string;
  /** Set on `harness_catalogue_changed` messages: the harness probed again. */
  harness?: string;
  /** Set on `cost_prices_synced` messages: how many price rows the sync wrote. */
  rows?: number;
  /** Set on `presence_*` messages: who arrived or left. */
  actor?: Actor;
  /** Set on `presence_*` messages: the Run's Presences once it happened. */
  presences?: Presence[];
  /** Set on `hosted_session_changed` and `hosted_session_forgotten`
   *  messages: the session as it now stands, or as it was last recorded. */
  session?: Partial<HostedSession> & { id: string };
  /** Set on `hosted_live_artifacts_changed` messages: the session's name. */
  id?: string;
  /** Set on `hosted_live_artifacts_changed` and `extension_event` messages:
   *  the extension it is about, or that published it. */
  extension?: string;
  /** Set on `hosted_live_artifacts_changed` messages: every Artefact live the
   *  session now has. */
  artifacts?: HostedLiveArtifact[];
}
