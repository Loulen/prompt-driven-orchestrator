/**
 * The **Alerte d'attente** decider (CONTEXT.md « Alerte d'attente »,
 * ADR-0100).
 *
 * Pure: one live `/ws` message, where the tabs stand, this browser's switches,
 * the permission and the clock in; an alert (a notification and/or a sound) or
 * nothing out. No DOM, no Notification API, no timer: the adapters only carry
 * out what this module decided. The Portée d'alerte is judged apart
 * (`alertScope.ts`), since it may need the daemon.
 *
 * Alert on a **live** transition to a human wait:
 * - `node_awaiting_user` of cause `declared` (the agent ran `pdo wait-user`) or
 *   `completion_not_released` (the daemon refused an unreleased completion);
 * - an incident park: `node_interrupted` or `run_interrupted`.
 *
 * Never on `children_pending` (the parent node works), on `child_awaiting`
 * (derived on read: the child alerts for itself), on `resync` (a reload or a
 * reconnection replays nothing: a wait already there does not alert), or when a
 * PDO tab of this browser is in view.
 */
import type { WsMessage } from "./liveStream";
import { chimeForReason, type ChimeName } from "./chimes";
import type { WaitingAlertPrefs } from "./alertScope";
import { brandName } from "../../shared/ui/brand/brand";

/** At most one sound per window, every reason and every tab together. */
export const SOUND_THROTTLE_MS = 10_000;

/**
 * What this browser lets a notification do. `insecure`: the page is not a
 * secure context (http through a LAN IP), where browsers refuse the API.
 * `unavailable`: no Notification API at all.
 */
export type AlertPermission = "default" | "granted" | "denied" | "insecure" | "unavailable";

/** Why the node or the Run waits: one notification text and one chime each. */
export type WaitingReason = "question" | "declared" | "completion" | "incident";

/** A live transition to a human wait, read off one `/ws` event. */
export interface WaitingTransition {
  reason: WaitingReason;
  runId: string;
  /** Absent for a park of the whole Run (`run_interrupted` names no node). */
  nodeId: string | null;
  iter: number | null;
  /** The agent's question (`question`) or the incident's reason (`incident`). */
  message: string | null;
}

/** The incident codes that are no incident to alert on (a budget reached waits on the region). */
const SILENT_PARKS: ReadonlySet<string> = new Set(["region_budget_exhausted"]);

function payloadString(payload: Record<string, unknown> | null | undefined, key: string): string | null {
  const value = payload?.[key];
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

/** An incident's reason reads `"<slug>: <prose>"`: the prose alone is for a person. */
export function incidentProse(reason: string | null): string | null {
  if (!reason) return null;
  const match = /^([a-z0-9_]+): (.+)$/s.exec(reason);
  return match ? match[2].trim() : reason;
}

function incidentCode(payload: Record<string, unknown> | null | undefined): string | null {
  const code = payloadString(payload, "reason_code");
  if (code) return code;
  const match = /^([a-z0-9_]+): /.exec(payloadString(payload, "reason") ?? "");
  return match ? match[1] : null;
}

/** The waiting transition a `/ws` message carries, or `null` when it is none. */
export function waitingTransitionOf(msg: WsMessage): WaitingTransition | null {
  if (msg.type !== "event" || !msg.event) return null;
  const { kind, run_id: runId, node_id: nodeId, iter, payload } = msg.event;
  if (!runId) return null;
  const base = { runId, nodeId: nodeId ?? null, iter: iter ?? null };

  if (kind === "node_awaiting_user") {
    if (!nodeId) return null;
    // The daemon's projection reads `cause`, else the binding marker's
    // `reason` slug, else a payload-less event as `declared`.
    const cause = payloadString(payload, "cause") ?? payloadString(payload, "reason") ?? "declared";
    if (cause === "declared") {
      const message = payloadString(payload, "message");
      return { ...base, reason: message ? "question" : "declared", message };
    }
    if (cause === "completion_not_released") return { ...base, reason: "completion", message: null };
    // `children_pending`, `child_awaiting`, any later cause: no human wait.
    return null;
  }
  if (kind === "node_interrupted" || kind === "run_interrupted") {
    if (kind === "node_interrupted" && !nodeId) return null;
    const code = incidentCode(payload);
    if (code && SILENT_PARKS.has(code)) return null;
    return { ...base, reason: "incident", message: incidentProse(payloadString(payload, "reason")) };
  }
  return null;
}

/** The events after which a node no longer waits on the wait it showed. */
const LIFTS: ReadonlySet<string> = new Set([
  "node_resumed",
  "node_completion_released",
  "node_completed",
  "node_failed",
  "node_stopped",
  "node_started",
  "node_auto_completed",
  "node_invalidated",
  "node_interrupted",
]);

/**
 * The waits this tab saw open. The daemon writes a `node_awaiting_user` again
 * when a node already waiting changes its question: a refresh, not a
 * transition, so no second alert. A lift closes the wait; a `resync` forgets
 * them all (a lift may have been missed while away).
 */
export function createWaitMemory() {
  const open = new Set<string>();
  return {
    /** Feed every message; `true` when it only refreshes a wait already open. */
    refreshes(msg: WsMessage): boolean {
      if (msg.type === "resync") {
        open.clear();
        return false;
      }
      const event = msg.type === "event" ? msg.event : undefined;
      if (!event?.node_id) return false;
      const node = `${event.run_id}:${event.node_id}:`;
      if (LIFTS.has(event.kind)) {
        for (const key of [...open]) if (key.startsWith(node)) open.delete(key);
        return false;
      }
      const t = event.kind === "node_awaiting_user" ? waitingTransitionOf(msg) : null;
      if (!t) return false;
      const key = notificationTag(t);
      if (open.has(key)) return true;
      open.add(key);
      return false;
    },
  };
}

export interface WaitingAlertInput {
  message: WsMessage;
  /** Is a PDO tab of this browser in view, this one or another? */
  anyTabVisible: boolean;
  prefs: Pick<WaitingAlertPrefs, "notification" | "sound">;
  permission: AlertPermission;
  /** Epoch ms. */
  now: number;
  /** When the last chime rang in any tab (epoch ms), `null` if never. */
  lastSoundAt: number | null;
}

export interface WaitingAlertDecision {
  transition: WaitingTransition;
  notify: boolean;
  /** Ring: already throttled to one per {@link SOUND_THROTTLE_MS}. */
  sound: boolean;
  /** The chime of the reason, `null` without sound. */
  chime: ChimeName | null;
}

/** Is the sound window open: no chime yet, or the last one 10 s ago or more? */
export function soundWindowOpen(lastSoundAt: number | null, now: number): boolean {
  return lastSoundAt === null || now - lastSoundAt >= SOUND_THROTTLE_MS;
}

/** The decider. `null`: nothing to do. */
export function decideWaitingAlert(input: WaitingAlertInput): WaitingAlertDecision | null {
  const transition = waitingTransitionOf(input.message);
  if (!transition || input.anyTabVisible) return null;
  const notify = input.prefs.notification && input.permission === "granted";
  const sound = input.prefs.sound && soundWindowOpen(input.lastSoundAt, input.now);
  if (!notify && !sound) return null;
  return { transition, notify, sound, chime: sound ? chimeForReason(transition.reason) : null };
}

/** The names the notification shows; the ids stand in for unknown ones. */
export interface WaitingAlertLabels {
  runName?: string | null;
  nodeName?: string | null;
}

export interface NotificationTarget {
  runId: string;
  nodeId: string | null;
}

export interface NotificationSpec {
  title: string;
  body: string;
  /** `run:node:iter` (the Run alone for a park of the Run): the browser replaces a duplicate. */
  tag: string;
  target: NotificationTarget;
}

export function notificationTag(t: Pick<WaitingTransition, "runId" | "nodeId" | "iter">): string {
  if (!t.nodeId) return t.runId;
  return `${t.runId}:${t.nodeId}:${t.iter ?? 1}`;
}

/** Title, body and tag of the notification of a transition. */
export function buildNotificationSpec(transition: WaitingTransition, labels: WaitingAlertLabels = {}): NotificationSpec {
  const run = labels.runName?.trim() || transition.runId;
  const node = labels.nodeName?.trim() || transition.nodeId || "";
  let title: string;
  let body: string;
  switch (transition.reason) {
    case "question":
      title = `${node} is waiting for you`;
      body = `${run} · Question · « ${transition.message} »`;
      break;
    case "declared":
      title = `${node} is waiting for you`;
      body = `${run} · Waiting for your answer`;
      break;
    case "completion":
      title = `${node} is done, waiting for your go`;
      body = `${run} · Approve to complete it`;
      break;
    case "incident":
      title = "Run paused on an incident";
      body = [run, node || null, transition.message].filter(Boolean).join(" · ");
      break;
  }
  return { title, body, tag: notificationTag(transition), target: { runId: transition.runId, nodeId: transition.nodeId } };
}

/** The notification « Test » shows. Its click only brings the tab back. */
export const TEST_NOTIFICATION = {
  title: `${brandName()} · test notification`,
  body: `This is how ${brandName()} tells you a node is waiting for you.`,
  tag: "pdo-waiting-alert-test",
} as const;
