/**
 * The **waiting alert** decider (#1011, CONTEXT.md « Alerte d'attente », ADR-0081).
 *
 * Pure: one live `/ws` message + where the tabs stand + this browser's
 * preferences + the clock → an alert (a notification and/or a sound) or
 * nothing. No DOM, no Notification API, no timer — the adapters
 * (`notificationAdapter`, and the sound one of #1012) only carry out what this
 * module decided, so the whole matrix is unit-tested here. #1012 adds the
 * chime of each reason (`chimes.ts`) and the visibility of every PDO tab of the
 * browser (`tabCoordinator.ts`).
 *
 * Alert on a **live** transition to a human wait:
 * - `node_awaiting_user` of cause `declared` (the agent ran `pdo wait-user`) or
 *   `completion_not_released` (the daemon refused an unreleased completion);
 * - an incident park: `node_interrupted` (session death, boot recovery, spawn
 *   abort) or `run_interrupted` (stall, merge conflict, `unrouted`, …).
 *
 * Never on `children_pending` (the orchestrator is working), on `child_awaiting`
 * (derived on read, the child alerts for itself), on `resync` (a reload or a
 * reconnection replays nothing — a wait already there does not alert), or when a
 * PDO tab is in view (the dot already says it).
 */
import type { WsMessage } from "../types";
import { chimeForReason, type ChimeName } from "./chimes";

/** At most one sound per window, every cause together (ADR-0081 §4). */
export const SOUND_THROTTLE_MS = 10_000;

/**
 * What this browser lets a notification do. `insecure`: the page is not a
 * secure context (http through a LAN IP) — the browser refuses the API there.
 * `unavailable`: no Notification API at all.
 */
export type AlertPermission = "default" | "granted" | "denied" | "insecure" | "unavailable";

/** The two independent switches, both off by default (Settings › Notifications). */
export interface WaitingAlertPrefs {
  notification: boolean;
  sound: boolean;
}

export const DEFAULT_WAITING_ALERT_PREFS: WaitingAlertPrefs = {
  notification: false,
  sound: false,
};

/**
 * Why the node / Run waits. One per sound of #1012 (`question` and `declared`
 * are both `declared` on the wire, told apart by the message).
 */
export type WaitingReason = "question" | "declared" | "completion" | "incident";

/** A live transition to a human wait, read off one `/ws` event. */
export interface WaitingTransition {
  reason: WaitingReason;
  runId: string;
  /** Absent for a run-level park (`run_interrupted` naming no node). */
  nodeId: string | null;
  iter: number | null;
  /** The agent's question (`question`) or the incident's reason (`incident`). */
  message: string | null;
}

function payloadString(payload: Record<string, unknown> | null, key: string): string | null {
  const value = payload?.[key];
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

/** Read a waiting transition off a `/ws` message, or `null` when it is none. */
export function waitingTransitionOf(msg: WsMessage): WaitingTransition | null {
  if (msg.type !== "event" || !msg.event) return null;
  const { kind, run_id: runId, node_id: nodeId, iter, payload } = msg.event;
  if (!runId) return null;
  const base = { runId, nodeId: nodeId ?? null, iter: iter ?? null };

  if (kind === "node_awaiting_user") {
    if (!nodeId) return null;
    // Same read as the daemon's projection: `cause`, else the binding marker's
    // `reason` slug, else a legacy payload-less event that reads as `declared`.
    const cause = payloadString(payload, "cause") ?? payloadString(payload, "reason") ?? "declared";
    if (cause === "declared") {
      const message = payloadString(payload, "message");
      return { ...base, reason: message ? "question" : "declared", message };
    }
    if (cause === "completion_not_released") {
      return { ...base, reason: "completion", message: null };
    }
    // `children_pending`, `child_awaiting`, any future cause: not a human wait.
    return null;
  }
  if (kind === "node_interrupted" || kind === "run_interrupted") {
    if (kind === "node_interrupted" && !nodeId) return null;
    return { ...base, reason: "incident", message: payloadString(payload, "reason") };
  }
  return null;
}

export interface WaitingAlertInput {
  message: WsMessage;
  /** Is a PDO tab of this browser in view — any of them (#1012)? */
  anyTabVisible: boolean;
  prefs: WaitingAlertPrefs;
  permission: AlertPermission;
  /** Epoch ms. */
  now: number;
  /** When the last sound played (epoch ms), `null` if never. */
  lastSoundAt: number | null;
}

export interface WaitingAlertDecision {
  transition: WaitingTransition;
  /** Show a notification. */
  notify: boolean;
  /** Play the sound — already throttled to one per {@link SOUND_THROTTLE_MS}. */
  sound: boolean;
  /** The chime of the transition's reason (`null` when no sound). */
  chime: ChimeName | null;
}

/** Is the sound window open — no chime yet, or the last one 10 s ago or more? */
export function soundWindowOpen(lastSoundAt: number | null, now: number): boolean {
  return lastSoundAt === null || now - lastSoundAt >= SOUND_THROTTLE_MS;
}

/** The decider. `null` = nothing to do. */
export function decideWaitingAlert(input: WaitingAlertInput): WaitingAlertDecision | null {
  const transition = waitingTransitionOf(input.message);
  if (!transition) return null;
  if (input.anyTabVisible) return null;
  const notify = input.prefs.notification && input.permission === "granted";
  const sound = input.prefs.sound && soundWindowOpen(input.lastSoundAt, input.now);
  if (!notify && !sound) return null;
  return { transition, notify, sound, chime: sound ? chimeForReason(transition.reason) : null };
}

/** The names the notification shows, resolved by the caller. */
export interface WaitingAlertLabels {
  /** The Run's display name; the id when unknown. */
  runName?: string | null;
  /** The node's display name; the id when unknown. */
  nodeName?: string | null;
}

export interface NotificationTarget {
  runId: string;
  nodeId: string | null;
}

export interface NotificationSpec {
  title: string;
  body: string;
  /** `run:node:iter` (just `run` for a run-level park): the browser replaces a duplicate. */
  tag: string;
  target: NotificationTarget;
}

export function notificationTag(t: Pick<WaitingTransition, "runId" | "nodeId" | "iter">): string {
  if (!t.nodeId) return t.runId;
  return `${t.runId}:${t.nodeId}:${t.iter ?? 1}`;
}

/** Title, body and tag of the notification (English, like the whole UI). */
export function buildNotificationSpec(
  transition: WaitingTransition,
  labels: WaitingAlertLabels = {},
): NotificationSpec {
  const runName = labels.runName?.trim() || transition.runId;
  const nodeName = labels.nodeName?.trim() || transition.nodeId || "";
  let body: string;
  switch (transition.reason) {
    case "question":
      body = transition.message ?? `Node ${nodeName} is waiting for you`;
      break;
    case "declared":
      body = `Node ${nodeName} is waiting for you`;
      break;
    case "completion":
      body = `Node ${nodeName} is done and waits for your go`;
      break;
    case "incident":
      body = transition.message
        ? `Run paused on an incident: ${transition.message}`
        : "Run paused on an incident";
      break;
  }
  return {
    title: `PDO — ${runName} is waiting for you`,
    body,
    tag: notificationTag(transition),
    target: { runId: transition.runId, nodeId: transition.nodeId },
  };
}

/** The « Send a test » notification. Its click only brings the tab back. */
export const TEST_NOTIFICATION = {
  title: "PDO — test notification",
  body: "This is how PDO tells you a node is waiting for you.",
  tag: "pdo-waiting-alert-test",
} as const;
