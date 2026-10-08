// The types of the shared module `@pdo/extension-ui` (ADR-0081, amendment #32),
// for the authors of extension bundles: copy this file into the bundle's
// sources as `pdo-extension-ui.d.ts` and wrap it in
// `declare module "@pdo/extension-ui" { … }`. The implementation is
// src/shared/extensions/uiModule.ts; docs/reference/extension-ui.md documents it.
import type { ReactNode } from "react";

export interface ExtensionRef {
  id: string;
  name: string;
}

/** What the host hands its elements, on top of every location's context. */
export type HostContext = Record<string, unknown>;

/** The context an element of a hosted location receives: the selected Run,
 *  its Projet, `location`, and the host's keys. */
export type HostedContext = HostContext & {
  location: string;
  run: { id: string; name: string | null; pipeline: string; status: string; attributes: Record<string, string> } | null;
  project: { id: string; name: string; attributes: Record<string, string> } | null;
};

export interface HostedElement {
  /** `<extension id>:<contribution id>`. */
  key: string;
  extension: ExtensionRef;
  id: string;
  label: string;
  type: "page" | "action" | "tab" | "section" | "decoration" | "list_filter";
  context: HostedContext;
}

/** Who triggers a mutation (since 0.8): the core's `Actor`, as on the wire. */
export type Actor =
  | { kind: "local_operator" }
  | { kind: "agent"; run_id: string; node_id: string }
  | { kind: "trigger"; trigger_id: string }
  | { kind: "identified"; namespace: string; id: string };

/** How an Actor shows, as an extension names it (since 0.8). */
export interface ActorDisplay {
  name: string;
  avatar_url: string | null;
}

/** The version of `pdo-extension-api` the module comes with, and its major. */
export const version: string;
export const major: number;

export function useHostedContributions(location: string, context?: HostContext): HostedElement[];
export function runHostedCommand(element: HostedElement): Promise<void>;
export function HostedContribution(props: {
  element: HostedElement;
  props?: Record<string, unknown>;
}): ReactNode;
export function HostedContributions(props: {
  location: string;
  context?: HostContext;
  props?: Record<string, unknown>;
  wrap?: (element: HostedElement, rendered: ReactNode) => ReactNode;
}): ReactNode;
/** The UI's own Actor and its display name; `null` until known (since 0.8). */
export function useCurrentActor(): { actor: Actor; display: ActorDisplay | null } | null;
/** The display name of `actor`; `null` while unknown or when no extension
 *  names it (since 0.8). */
export function useActorDisplay(actor: Actor | null | undefined): ActorDisplay | null;

/** Where a probed socket stands (since 0.10): probing or connecting for the
 *  first time, open, or down (probing again, or closed for good). */
export type ProbedSocketState = "connexion" | "ouvert" | "hors_ligne";

/** A WebSocket to the daemon, opened through the probe (since 0.10). */
export interface ProbedSocket {
  readonly state: ProbedSocketState;
  /** Send when open; whether it was sent. */
  send(data: string | ArrayBuffer | ArrayBufferView<ArrayBuffer> | Blob): boolean;
  /** Every message: text as is, binary as an `ArrayBuffer`. Returns the unsubscribe. */
  onMessage(listener: (data: string | ArrayBuffer) => void): () => void;
  /** Every change of state (`ouvert` at each opening). Returns the unsubscribe. */
  onState(listener: (state: ProbedSocketState) => void): () => void;
  /** Close for good, with 1000. */
  close(): void;
}

/** Open `path` (`/ext/<id>/…`) on the daemon of the page, only after an HTTP
 *  probe succeeded; while it fails, the probe repeats (backoff with jitter), a
 *  hidden tab does not probe, a refused session goes to the sign-in page.
 *  `reconnect: false`: never reopened once it ended; `"after-drop"`: reopened
 *  only when it dropped without the daemon's Close. Since 0.10.
 *  `silenceMs` (since 0.11): an open socket that received nothing for that
 *  long, or whose browser went offline, is closed and counts as a drop; back
 *  online, a pending probe is sent at once. */
export function openProbedSocket(
  path: string,
  options?: { reconnect?: boolean | "after-drop"; silenceMs?: number },
): ProbedSocket;

declare const ExtensionUi: {
  version: string;
  major: number;
  useHostedContributions: typeof useHostedContributions;
  runHostedCommand: typeof runHostedCommand;
  HostedContribution: typeof HostedContribution;
  HostedContributions: typeof HostedContributions;
  useCurrentActor: typeof useCurrentActor;
  useActorDisplay: typeof useActorDisplay;
  openProbedSocket: typeof openProbedSocket;
};
export default ExtensionUi;
