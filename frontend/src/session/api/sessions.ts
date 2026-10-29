// The session context's routes: the status bar's gauges, a node's pane, the
// OS terminal pop-outs, the run shell, a text written into a node's input and
// the Zoom du terminal. The terminal itself is the
// `WS /sessions/<session>/pty` bridge (see `components/TmuxTerminal`).

import type { DaemonStatus } from "../../platform/model/daemon";
import type { TerminalTextResponse } from "../../shared/generated/TerminalTextResponse";
import type { TerminalZoomResponse } from "../../shared/generated/TerminalZoomResponse";
import { request } from "../../shared/http/client";
import { posteId } from "../model/poste";

export function fetchSessions(): Promise<DaemonStatus> {
  return request<DaemonStatus>("GET", "/sessions");
}

export function attachSession(sessionId: string): Promise<void> {
  return request<void>(
    "POST",
    `/sessions/${encodeURIComponent(sessionId)}/attach`,
    { responseMode: "void", label: "attach" },
  );
}

export function attachManager(runId: string): Promise<void> {
  return request<void>(
    "POST",
    `/sessions/${encodeURIComponent(runId)}/manager/attach`,
    { responseMode: "void", label: "manager attach" },
  );
}

/**
 * Open (or re-attach) an ad-hoc bash shell in a terminal run's pipeline
 * worktree (#316 / ADR-0021). Create-if-absent; returns the tmux session name to
 * attach to via the existing `WS /sessions/<session>/pty` bridge (no OS spawn).
 * `created` distinguishes a fresh shell from a re-attach.
 */
export function openRunShell(
  runId: string,
): Promise<{ session: string; created: boolean }> {
  return request<{ session: string; created: boolean }>(
    "POST",
    `/sessions/${encodeURIComponent(runId)}/shell`,
    { label: "open shell" },
  );
}

export interface PaneResponse {
  content: string;
  session_name: string;
  resumed: boolean;
  stale: boolean;
  /**
   * Provenance of `content` (#205): "live" (captured from a running session),
   * "resumed" (a dead latest-iter session was re-attached), "snapshot" (the
   * persisted post-mortem pane of a reaped terminal node), or "unavailable"
   * (no session and no snapshot).
   */
  source: "live" | "resumed" | "snapshot" | "unavailable";
}

export function fetchPane(
  runId: string,
  nodeId: string,
  iter: number,
): Promise<PaneResponse> {
  return request<PaneResponse>(
    "GET",
    `/runs/${encodeURIComponent(runId)}/nodes/${encodeURIComponent(nodeId)}/pane`,
    { query: { iter }, label: "GET pane" },
  );
}

/** Write `text` into the input of a node's live session **without** pressing
 *  Enter: the user completes and submits it in the terminal. The request names
 *  this browser's poste: the daemon refuses it (`409 not_pilot`) while another
 *  poste pilots the terminal (ADR-0075). */
export function sendTextToNodeTerminal(
  runId: string,
  nodeId: string,
  iter: number,
  text: string,
): Promise<TerminalTextResponse> {
  return request<TerminalTextResponse>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/nodes/${encodeURIComponent(nodeId)}/terminal-text`,
    { body: { iter, text, poste: posteId() }, label: "Send to terminal" },
  );
}

/** The Zoom du terminal of a session (CORE-124): `hosted` says whether the
 *  session carries one (a Session hébergée); any other is at 100 %. */
export function fetchTerminalZoom(session: string): Promise<TerminalZoomResponse> {
  return request<TerminalZoomResponse>(
    "GET",
    `/sessions/${encodeURIComponent(session)}/terminal-zoom`,
    { label: "terminal zoom" },
  );
}

/** Set the Zoom du terminal of a Session hébergée, for every Poste on it. */
export function putTerminalZoom(session: string, zoom: number): Promise<TerminalZoomResponse> {
  return request<TerminalZoomResponse>(
    "PUT",
    `/sessions/${encodeURIComponent(session)}/terminal-zoom`,
    { body: { zoom }, label: "Terminal zoom" },
  );
}
