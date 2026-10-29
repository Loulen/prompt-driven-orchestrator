// The routes of a Session hébergée named by its id (CORE-125): what the detail
// of a session reads and does, each the extension's gesture on it.

import type { HostedSessionEvent } from "../../shared/generated/HostedSessionEvent";
import { request } from "../../shared/http/client";
import type { HostedConfig, HostedLiveArtifact, HostedSession, HostedSessionView } from "../model/hostedSession";

const base = (id: string) => `/sessions/${encodeURIComponent(id)}`;

/** The session and who drives its terminal. */
export function fetchHostedSession(id: string): Promise<HostedSessionView> {
  return request<HostedSessionView>("GET", `${base(id)}/hosted`, { label: "hosted session" });
}

export function terminateHostedSession(id: string): Promise<HostedSession> {
  return request<HostedSession>("POST", `${base(id)}/terminate`, { label: "Terminate" });
}

export function relaunchHostedSession(id: string): Promise<HostedSession> {
  return request<HostedSession>("POST", `${base(id)}/relaunch`, { label: "Relaunch" });
}

/** Replace its whole configuration, for its next relaunch. */
export function configureHostedSession(id: string, config: HostedConfig): Promise<HostedSession> {
  return request<HostedSession>("PUT", `${base(id)}/config`, { body: config, label: "Configuration" });
}

/** Its journal, oldest first. */
export function fetchHostedSessionEvents(id: string): Promise<HostedSessionEvent[]> {
  return request<HostedSessionEvent[]>("GET", `${base(id)}/events`, { label: "session events" });
}

export function fetchHostedLiveArtifacts(id: string): Promise<HostedLiveArtifact[]> {
  return request<HostedLiveArtifact[]>("GET", `${base(id)}/live`, { label: "live artifacts" });
}

/** Where its Artefact live `path` is served; `download`: as an attachment. */
export function hostedLiveArtifactUrl(id: string, path: string, download = false): string {
  const encoded = path.split("/").map(encodeURIComponent).join("/");
  return `${base(id)}/live/${encoded}${download ? "?download" : ""}`;
}

export function fetchHostedLiveArtifactText(id: string, path: string): Promise<string> {
  return request<string>("GET", hostedLiveArtifactUrl(id, path), { responseMode: "text", label: path });
}
