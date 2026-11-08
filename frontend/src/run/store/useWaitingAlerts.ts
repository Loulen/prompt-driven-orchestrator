/**
 * The controller of the Alerte d'attente: listens to the live `/ws` stream,
 * asks the pure decider (`run/model/waitingAlert.ts`) what to do with each
 * message, judges the Portée d'alerte (`run/model/alertScope.ts`), and has the
 * adapters carry it out. Mounted once, in `App`.
 *
 * The decision is taken at the event: the visibility of every PDO tab of this
 * browser, the switches, the permission and the sound window are read then.
 * The Portée may need the daemon (a Run launched a second ago is not listed
 * yet). Then the tab claims the transition: every tab sees the same event, and
 * only the one whose claim wins emits. The sound window is read again once the
 * claim is won: another tab may have just rung.
 */
import { useEffect, useRef } from "react";
import type { Actor } from "../../shared/generated/Actor";
import type { RunListEntry } from "../../shared/generated/RunListEntry";
import type { RunState } from "../../shared/generated/RunState";
import { useActorsStore } from "../../shared/actors/store";
import { followDeepLink, formatNodeLink, formatRunLink } from "../../shared/extensions/deepLink";
import { fetchRun } from "../api/runs";
import { resolveRoot, scopeAllows, type RunLineage } from "../model/alertScope";
import type { WsMessage } from "../model/liveStream";
import {
  buildNotificationSpec,
  createWaitMemory,
  decideWaitingAlert,
  notificationTag,
  soundWindowOpen,
  type NotificationTarget,
  type WaitingAlertDecision,
  type WaitingTransition,
} from "../model/waitingAlert";
import { showNotification } from "./notificationAdapter";
import { canPlaySoundNow, playChime } from "./soundAdapter";
import { getTabCoordinator } from "./tabCoordinatorBrowser";
import { getWaitingAlertSettings } from "./waitingAlertSettings";

/** What the controller reads besides the stream; injected so a test drives it. */
export interface WaitingAlertSources {
  /** The Runs the list holds. */
  runs: () => RunListEntry[];
  /** `GET /runs/{id}`. */
  fetchRun: (runId: string) => Promise<RunState>;
  /** The UI's own Actor, `null` until known. */
  me: () => Actor | null;
}

const runName = (run: { name?: string | null; pipeline_name?: string }) =>
  run.name?.trim() || run.pipeline_name?.trim() || null;

/**
 * The claim of one transition: its `run:node:iter` and the event that carried
 * it (two questions in a row of the same iteration are two alerts).
 */
function claimKey(message: WsMessage, t: WaitingTransition): string {
  const event = message.event;
  return `${notificationTag(t)}#${event?.id ?? event?.ts ?? ""}#${event?.kind ?? ""}`;
}

/** The Run's state, asked once per transition. */
function lazyRun(sources: WaitingAlertSources, runId: string): () => Promise<RunState | null> {
  let pending: Promise<RunState | null> | null = null;
  return () => (pending ??= sources.fetchRun(runId).catch(() => null));
}

async function inScope(sources: WaitingAlertSources, t: WaitingTransition, run: () => Promise<RunState | null>) {
  const { prefs } = getWaitingAlertSettings();
  if (prefs.scope === "all") return true;
  const listed = (id: string): RunLineage | undefined => sources.runs().find((r) => r.run_id === id);
  const ask = (id: string): Promise<RunLineage | null> => (id === t.runId ? run() : sources.fetchRun(id));
  const root = await resolveRoot(t.runId, listed, ask);
  return scopeAllows(getWaitingAlertSettings().prefs, root, sources.me());
}

async function emit(
  sources: WaitingAlertSources,
  decision: WaitingAlertDecision,
  run: () => Promise<RunState | null>,
  onOpen: (target: NotificationTarget) => void,
): Promise<void> {
  const tabs = getTabCoordinator();
  const { transition: t, chime } = decision;
  if (chime && canPlaySoundNow() && soundWindowOpen(tabs.lastSoundAt(), Date.now())) {
    // The chime is claimed on its own, on a key every tab derives from the same
    // last chime: two transitions won by two tabs in a burst still ring once.
    // A tab the browser would not let ring claims nothing: its claim would hold
    // the key while no chime moves it on.
    void tabs.claim(`sound@${tabs.lastSoundAt() ?? "none"}`).then((won) => {
      const now = Date.now();
      if (won && soundWindowOpen(tabs.lastSoundAt(), now) && playChime(chime)) tabs.noteSound(now);
    });
  }
  if (!decision.notify) return;
  const listed = sources.runs().find((r) => r.run_id === t.runId);
  const state = !listed || t.nodeId ? await run() : null;
  const nodeName = t.nodeId ? state?.node_defs?.find((d) => d.id === t.nodeId)?.name?.trim() : null;
  const spec = buildNotificationSpec(t, { runName: (listed && runName(listed)) || (state && runName(state)), nodeName });
  showNotification(spec, () => onOpen(spec.target));
}

/** The waits this page saw open: a refreshed question alerts once. */
let waits = createWaitMemory();

/** Test seam: forget the waits seen. */
export function resetWaitMemory(): void {
  waits = createWaitMemory();
}

/** Handle one message of the stream. Exported for the tests. */
export function handleWaitingAlert(
  message: WsMessage,
  sources: WaitingAlertSources,
  onOpen: (target: NotificationTarget) => void,
): Promise<void> {
  if (waits.refreshes(message)) return Promise.resolve();
  const tabs = getTabCoordinator();
  const { prefs, permission } = getWaitingAlertSettings();
  const decision = decideWaitingAlert({
    message,
    anyTabVisible: tabs.anyTabVisible(),
    prefs,
    permission,
    now: Date.now(),
    lastSoundAt: tabs.lastSoundAt(),
  });
  if (!decision) return Promise.resolve();
  const run = lazyRun(sources, decision.transition.runId);
  return inScope(sources, decision.transition, run)
    .then((allowed) => (allowed ? tabs.claim(claimKey(message, decision.transition)) : false))
    .then((won) => (won ? emit(sources, decision, run, onOpen) : undefined))
    .catch(() => {});
}

export interface UseWaitingAlertsOptions {
  subscribe: (listener: (msg: WsMessage) => void) => () => void;
  runs: RunListEntry[];
  /** Close what covers the Workbench (Settings through its guard), then `then`. */
  leaveSurfaces: (then: () => void) => void;
}

/**
 * The notification's click: the tab is already in front; the Run opens on the
 * node through its deep link (`#/runs/<run>/nodes/<node>`), as any link would.
 */
export function openAlertTarget(target: NotificationTarget): void {
  followDeepLink(target.nodeId ? formatNodeLink(target.runId, target.nodeId) : formatRunLink(target.runId));
}

export function useWaitingAlerts({ subscribe, runs, leaveSurfaces }: UseWaitingAlertsOptions): void {
  // Latest-value refs: the listener subscribes once and reads the current list
  // and surfaces when an event arrives.
  const runsRef = useRef(runs);
  const leaveRef = useRef(leaveSurfaces);
  useEffect(() => {
    runsRef.current = runs;
    leaveRef.current = leaveSurfaces;
  }, [runs, leaveSurfaces]);
  useEffect(() => {
    const sources: WaitingAlertSources = {
      runs: () => runsRef.current,
      fetchRun,
      me: () => useActorsStore.getState().current?.actor ?? null,
    };
    const open = (target: NotificationTarget) => leaveRef.current(() => openAlertTarget(target));
    return subscribe((message) => void handleWaitingAlert(message, sources, open));
  }, [subscribe]);
}
