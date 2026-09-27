// The session context's routes: the status bar's gauges, a node's pane, the
// OS terminal pop-outs and the run shell. The terminal itself is the
// `WS /sessions/<session>/pty` bridge (see `components/TmuxTerminal`).

import type { DaemonStatus } from "../../platform/model/daemon";
import { request } from "../../shared/http/client";

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
