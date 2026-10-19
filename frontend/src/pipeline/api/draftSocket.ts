/**
 * The socket of a pipeline's shared draft (ADR-0088), `WS
 * /pipelines/{id}/draft`, opened through the probe (ADR-0083).
 *
 * Binary frames carry the replicated document, their first byte saying what
 * follows (the daemon's `adapters/http/draft.rs` speaks the same):
 * {@link SYNC_REQUEST} + a state vector, {@link SYNC_REPLY} + what the peer
 * missed, {@link UPDATE} + a change. Text frames are JSON: the daemon's
 * {@link DraftServerMessage}, the client's {@link DraftClientMessage}.
 */

import type { DraftClientMessage } from "../../shared/generated/DraftClientMessage";
import type { DraftServerMessage } from "../../shared/generated/DraftServerMessage";
import { openProbedSocket, type ProbedSocketState } from "../../shared/http/probedSocket";

export const SYNC_REQUEST = 0;
export const SYNC_REPLY = 1;
export const UPDATE = 2;

/** The daemon beats every 5 s: three missed beats make a dead socket. */
export const DRAFT_SILENCE_MS = 15_000;

export interface DraftSocketHandlers {
  /** The socket (re)opened: ask for what we miss. */
  onOpen: () => void;
  onBinary: (tag: number, payload: Uint8Array) => void;
  onMessage: (message: DraftServerMessage) => void;
  onState: (state: ProbedSocketState) => void;
}

export interface DraftSocket {
  sendBinary: (tag: number, payload: Uint8Array) => boolean;
  send: (message: DraftClientMessage) => boolean;
  close: () => void;
}

export function draftPath(pipelineId: string): string {
  return `/pipelines/${encodeURIComponent(pipelineId)}/draft`;
}

export function openDraftSocket(pipelineId: string, handlers: DraftSocketHandlers): DraftSocket {
  const socket = openProbedSocket(draftPath(pipelineId), { silenceMs: DRAFT_SILENCE_MS });
  socket.onState((state) => {
    handlers.onState(state);
    if (state === "ouvert") handlers.onOpen();
  });
  socket.onMessage((data) => {
    if (typeof data === "string") {
      try {
        handlers.onMessage(JSON.parse(data) as DraftServerMessage);
      } catch {
        // Not ours: ignored.
      }
      return;
    }
    const bytes = new Uint8Array(data);
    if (bytes.length === 0) return;
    handlers.onBinary(bytes[0], bytes.subarray(1));
  });
  return {
    sendBinary(tag, payload) {
      const frame = new Uint8Array(payload.length + 1);
      frame[0] = tag;
      frame.set(payload, 1);
      return socket.send(frame);
    },
    send(message) {
      return socket.send(JSON.stringify(message));
    },
    close() {
      socket.close();
    },
  };
}
