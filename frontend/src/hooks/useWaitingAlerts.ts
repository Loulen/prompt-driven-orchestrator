/**
 * The waiting alert controller (#1011, ADR-0081): listens to the live `/ws`
 * stream, asks the pure decider (`lib/waitingAlert.ts`) what to do with each
 * message, and has the adapters carry it out. Mounted once, in `App`.
 *
 * The decision is taken at the event, synchronously: visibility, switches,
 * permission and the sound window are read at that instant. Only the labels
 * (the Run's and the node's names) may need a fetch before the notification
 * shows — a Run launched a second ago may not be in the list yet.
 */
import { useEffect, useRef } from "react";
import { fetchRun } from "../api";
import type { RunListEntry, RunState, WsMessage } from "../types";
import { showNotification } from "../lib/notificationAdapter";
import {
  buildNotificationSpec,
  decideWaitingAlert,
  type NotificationTarget,
  type WaitingAlertLabels,
  type WaitingTransition,
} from "../lib/waitingAlert";
import { getWaitingAlertSettings } from "./useWaitingAlertSettings";

function runDisplayName(run: Pick<RunListEntry, "name" | "pipeline_name">): string | null {
  return run.name?.trim() || run.pipeline_name?.trim() || null;
}

function nodeDisplayName(run: Pick<RunState, "node_defs"> | null, nodeId: string): string | null {
  return run?.node_defs?.find((d) => d.id === nodeId)?.name?.trim() || null;
}

/** Does the body need the node's name (no agent message to show instead)? */
function needsNodeName(t: WaitingTransition): boolean {
  return !!t.nodeId && (t.reason === "declared" || t.reason === "completion" || (t.reason === "question" && !t.message));
}

async function resolveLabels(
  t: WaitingTransition,
  runs: RunListEntry[],
  openRun: RunState | null,
): Promise<WaitingAlertLabels> {
  const listed = runs.find((r) => r.run_id === t.runId);
  const known = openRun?.run_id === t.runId ? openRun : null;
  let runName = listed ? runDisplayName(listed) : known ? runDisplayName(known) : null;
  let nodeName = t.nodeId ? nodeDisplayName(known, t.nodeId) : null;
  if (!runName || (needsNodeName(t) && !nodeName)) {
    try {
      const run = await fetchRun(t.runId);
      runName = runName ?? runDisplayName(run);
      if (t.nodeId) nodeName = nodeName ?? nodeDisplayName(run, t.nodeId);
    } catch {
      // the ids stand in for the names
    }
  }
  return { runName, nodeName };
}

export interface UseWaitingAlertsOptions {
  subscribe: (listener: (msg: WsMessage) => void) => () => void;
  runs: RunListEntry[];
  /** The Run open in this tab, if any — a source of node names. */
  openRun: RunState | null;
  /** The notification was clicked: the tab is back in front, open this Run / node. */
  onOpen: (target: NotificationTarget) => void;
}

export function useWaitingAlerts({ subscribe, runs, openRun, onOpen }: UseWaitingAlertsOptions): void {
  // Latest-value refs: the WS listener stays subscribed once, and reads the
  // current list / open Run / opener when an event arrives.
  const runsRef = useRef(runs);
  const openRunRef = useRef(openRun);
  const onOpenRef = useRef(onOpen);
  useEffect(() => {
    runsRef.current = runs;
    openRunRef.current = openRun;
    onOpenRef.current = onOpen;
  }, [runs, openRun, onOpen]);
  // The sound window (one sound per 10 s, every cause together). #1012 plays
  // the sound; the decision and its throttle are already taken here.
  const lastSoundAtRef = useRef<number | null>(null);

  useEffect(() => {
    return subscribe((message) => {
      const { prefs, permission } = getWaitingAlertSettings();
      const now = Date.now();
      const decision = decideWaitingAlert({
        message,
        // #1012 aggregates the visibility of every PDO tab of this browser.
        anyTabVisible: document.visibilityState === "visible",
        prefs,
        permission,
        now,
        lastSoundAt: lastSoundAtRef.current,
      });
      if (!decision) return;
      if (decision.sound) lastSoundAtRef.current = now;
      if (!decision.notify) return;
      const { transition } = decision;
      void resolveLabels(transition, runsRef.current, openRunRef.current).then((labels) => {
        const spec = buildNotificationSpec(transition, labels);
        showNotification(spec, () => onOpenRef.current(spec.target));
      });
    });
  }, [subscribe]);
}
