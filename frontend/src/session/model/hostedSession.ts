// A Session hébergée as the detail of a session reads it (CORE-125): what the
// daemon answers on `/sessions/<id>/hosted` and publishes on `/ws`. Hand-written:
// the types live in the extension API, which `@pdo/extension-ui`'s `.d.ts`
// mirrors for the extensions.

import type { Actor } from "../../shared/generated/Actor";
import type { StatusTone } from "../../shared/ui/kit/tones";

export type HostedKind = "agent" | "shell";
export type HostedState = "starting" | "running" | "awaiting_user" | "ended";
export type EndReason = "terminated" | "exited";

/** How an agent session is launched; each field left out takes the
 *  instance's default. */
export interface HostedConfig {
  harness?: string | null;
  model?: string | null;
  effort?: string | null;
  prompt?: string | null;
}

export type HostedFolder = { kind: "path"; path: string } | { kind: "project_repo" } | { kind: "home" };

export interface HostedSession {
  /** Its tmux session's name: what its terminal is opened by. */
  id: string;
  extension: string;
  key: string;
  kind: HostedKind;
  config: HostedConfig;
  folder: HostedFolder;
  cwd: string;
  project: string | null;
  started_by: Actor | null;
  state: HostedState;
  end_reason: EndReason | null;
  wait_message?: string | null;
  terminal_zoom: number;
  created_at: string;
  updated_at: string;
}

/** `GET /sessions/<id>/hosted`: the session, and who drives its terminal. */
export interface HostedSessionView extends HostedSession {
  /** The Actor of the Pilote, or of the one Poste; `null` while no one shows it. */
  driving: Actor | null;
}

/** One Artefact live: a file the session's agent wrote in its `PDO_LIVE_DIR`. */
export interface HostedLiveArtifact {
  /** Under `PDO_LIVE_DIR`, `/`-separated. */
  path: string;
  size: number;
  modified_at: string;
}

/** Whether it holds a process. */
export function isLive(session: Pick<HostedSession, "state">): boolean {
  return session.state !== "ended";
}

/** Its state in words: `Running`, `Waiting for you`, `Terminated`… */
export function stateLabel(session: Pick<HostedSession, "state" | "end_reason">): string {
  switch (session.state) {
    case "starting":
      return "Starting";
    case "running":
      return "Running";
    case "awaiting_user":
      return "Waiting for you";
    case "ended":
      return session.end_reason === "exited" ? "Exited" : "Terminated";
  }
}

/** The tone its state is drawn in. */
export function stateTone(session: Pick<HostedSession, "state">): StatusTone {
  switch (session.state) {
    case "starting":
    case "running":
      return "running";
    case "awaiting_user":
      return "awaiting";
    case "ended":
      return "skipped";
  }
}

/** `Agent`, `Shell`. */
export function kindLabel(kind: HostedKind): string {
  return kind === "agent" ? "Agent" : "Shell";
}

/** Where it works, as a person reads it: `~` for the home folder. */
export function folderLabel(session: Pick<HostedSession, "folder" | "cwd">): string {
  return session.folder.kind === "home" ? "~" : session.cwd;
}

/** `45 s`, `12 min`, `2 h 14 min`, `3 d 4 h`: how long since `iso`. */
export function durationSince(iso: string, now: number = Date.now()): string {
  const then = Date.parse(iso);
  if (Number.isNaN(then)) return "";
  const seconds = Math.max(0, Math.floor((now - then) / 1000));
  if (seconds < 60) return `${seconds} s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return minutes % 60 ? `${hours} h ${minutes % 60} min` : `${hours} h`;
  const days = Math.floor(hours / 24);
  return hours % 24 ? `${days} d ${hours % 24} h` : `${days} d`;
}

/** The journal's entries that change its state (a configuration does not). */
const STATE_CHANGES = new Set(["opened", "running", "awaiting_user", "resumed", "terminated", "exited", "relaunched"]);

/** When it entered its state: the last such entry of its journal, its last
 *  change otherwise. */
export function stateSince(session: Pick<HostedSession, "updated_at">, journal: { at: string; kind: string }[]): string {
  for (let i = journal.length - 1; i >= 0; i -= 1) {
    if (STATE_CHANGES.has(journal[i].kind)) return journal[i].at;
  }
  return session.updated_at;
}

/** How an Artefact live previews in the detail. */
export type LivePreview = "markdown" | "image" | "text" | "none";

const IMAGE = new Set(["png", "jpg", "jpeg", "gif", "webp", "svg"]);
const TEXT = new Set(["txt", "log", "json", "yaml", "yml", "csv", "toml", "xml", "diff", "patch", "sh", "py", "ts", "js", "rs"]);

export function livePreview(path: string): LivePreview {
  const name = path.split("/").pop() ?? path;
  const dot = name.lastIndexOf(".");
  const ext = dot > 0 ? name.slice(dot + 1).toLowerCase() : "";
  if (ext === "md" || ext === "markdown") return "markdown";
  if (IMAGE.has(ext)) return "image";
  if (TEXT.has(ext) || ext === "") return "text";
  return "none";
}

/** The newest `modified_at` of `artifacts`, if any. */
export function newestModification(artifacts: HostedLiveArtifact[]): string | null {
  let newest: string | null = null;
  let newestMs = -Infinity;
  for (const artifact of artifacts) {
    const ms = Date.parse(artifact.modified_at);
    if (!Number.isNaN(ms) && ms > newestMs) {
      newestMs = ms;
      newest = artifact.modified_at;
    }
  }
  return newest;
}
