// The library pipeline authoring assistant's routes (ADR-0048, ADR-0051).

import { request } from "../../shared/http/client";

/**
 * Open (or re-attach) the library pipeline authoring assistant (#302 / ADR-0048,
 * #594 / ADR-0051). Create-if-absent; returns the shared tmux session name to
 * attach to via the existing `WS /sessions/<session>/pty` bridge.
 *
 * No pipeline id: there is **one** assistant for the whole daemon, and which
 * template it works on travels through {@link putLibassistFocus} instead.
 */
export function openLibraryAssistant(): Promise<{
  session: string;
  created: boolean;
}> {
  return request<{ session: string; created: boolean }>(
    "POST",
    "/sessions/libassist",
    { label: "open assistant" },
  );
}

/**
 * Reap the library authoring assistant (#302 / ADR-0048, #594 / ADR-0051).
 *
 * Called when the user leaves **every** pipeline edit view — not when they leave
 * the Assistant tab, which used to throw the conversation away on each round trip
 * between two templates. Best-effort: reaping an absent session is a no-op.
 *
 * `keepalive` lets the request survive the document, which is the only way the
 * `pagehide` path can send anything at all.
 */
export function closeLibraryAssistant(
  opts: { keepalive?: boolean } = {},
): Promise<{ ok: boolean; reaped: boolean }> {
  return request<{ ok: boolean; reaped: boolean }>(
    "DELETE",
    "/sessions/libassist",
    { label: "close assistant", keepalive: opts.keepalive },
  );
}

/**
 * Declare which template the UI is editing — or `null` to clear it (#594 /
 * ADR-0051). Sent on every edit-view change and repeated as a heartbeat.
 *
 * Two jobs in one call: it is how the assistant learns the open pipeline on its
 * next message, and it is how the daemon's reaper knows a human is still editing
 * even when no terminal is attached. The daemon resolves the instance-owned
 * pipeline path from its id.
 */
export function putLibassistFocus(pipelineId: string | null): Promise<unknown> {
  return request("PUT", "/sessions/libassist/focus", {
    body: { pipeline_id: pipelineId },
    label: "declare assistant focus",
  });
}
