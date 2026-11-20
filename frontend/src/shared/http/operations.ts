/**
 * The Opérations en cours: every mutation sent to the daemon (POST, PUT,
 * PATCH, DELETE) and not finished yet, named by a short label. The HTTP
 * client ({@link request} of `./client`) records each of its mutations here;
 * an extension records its own through `trackOperation` of
 * `@pdo/extension-ui`. Reads are never recorded.
 *
 * The status bar shows an operation once it has lasted {@link SHOW_AFTER_MS};
 * its end says the outcome in a toast: a failure always (the daemon's
 * message), a success only when it lasted more than
 * {@link SUCCESS_TOAST_AFTER_MS}.
 */

import { create } from "zustand";
import { showToast } from "../ui/toast";

/** An operation shorter than this shows nowhere but on its own button. */
export const SHOW_AFTER_MS = 300;
/** A success is said in a toast only past this duration. */
export const SUCCESS_TOAST_AFTER_MS = 2000;

export interface Operation {
  id: number;
  /** Present tense, short: « Archiving run ». */
  label: string;
  /** `Date.now()` at its start. */
  startedAt: number;
  /** Lasted {@link SHOW_AFTER_MS}: the status bar counts it. */
  visible: boolean;
  /** An extension's icon (an SVG URL) for its row; a spinner otherwise. */
  icon?: string;
}

export interface TrackOptions {
  /** Past tense, for the success toast: « Pipeline saved ». Default: « Done: <label> ». */
  done?: string;
  /** An extension's icon (an SVG URL) for its row in the status bar's list. */
  icon?: string;
}

/** The label of a mutation that names none, after its method. */
export function fallbackLabel(method: string): string {
  switch (method.toUpperCase()) {
    case "PUT":
    case "PATCH":
      return "Updating…";
    case "DELETE":
      return "Deleting…";
    default:
      return "Saving…";
  }
}

/** Whether a request of `method` changes something: only those are operations. */
export function isMutation(method: string): boolean {
  return ["POST", "PUT", "PATCH", "DELETE"].includes(method.toUpperCase());
}

interface OperationsState {
  operations: Operation[];
}

export const useOperationsStore = create<OperationsState>(() => ({ operations: [] }));

let nextId = 1;
const timers = new Map<number, ReturnType<typeof setTimeout>>();
/** The errors whose toast the tracker already showed: a caller does not say them twice. */
const shown = new WeakSet<object>();

/** Whether the failure `error` was already said in a toast by its operation. */
export function errorAlreadyShown(error: unknown): boolean {
  return typeof error === "object" && error !== null && shown.has(error);
}

/** The label without its trailing ellipsis, to run into a sentence. */
function bare(label: string): string {
  return label.replace(/(…|\.\.\.)$/, "").trim();
}

function seconds(ms: number): string {
  return `${Math.max(1, Math.round(ms / 1000))} s`;
}

/** The daemon's message of `error`, without a leading « <label> failed: » the client may have put. */
function messageOf(error: unknown, label: string): string {
  const raw = error instanceof Error ? error.message : String(error);
  const prefix = `${label} failed: `;
  return raw.startsWith(prefix) ? raw.slice(prefix.length) : raw;
}

/**
 * Record the start of an operation named `label`; call the function it
 * returns once it ends, with its error if it failed (`quiet`: a failure its
 * caller says itself, no toast). Ending twice does nothing more.
 */
export function beginOperation(
  label: string,
  options: TrackOptions = {},
): (error?: unknown, quiet?: boolean) => void {
  const id = nextId++;
  const startedAt = Date.now();
  useOperationsStore.setState((s) => ({
    operations: [...s.operations, { id, label, startedAt, visible: false, icon: options.icon }],
  }));
  timers.set(
    id,
    setTimeout(() => {
      timers.delete(id);
      useOperationsStore.setState((s) => ({
        operations: s.operations.map((o) => (o.id === id ? { ...o, visible: true } : o)),
      }));
    }, SHOW_AFTER_MS),
  );
  let ended = false;
  return (error?: unknown, quiet = false) => {
    if (ended) return;
    ended = true;
    clearTimeout(timers.get(id));
    timers.delete(id);
    useOperationsStore.setState((s) => ({ operations: s.operations.filter((o) => o.id !== id) }));
    const elapsed = Date.now() - startedAt;
    if (quiet) return;
    if (error !== undefined) {
      if (typeof error === "object" && error !== null) shown.add(error);
      showToast({ kind: "error", title: `${bare(label)} failed`, message: messageOf(error, bare(label)) });
    } else if (elapsed > SUCCESS_TOAST_AFTER_MS) {
      showToast({ kind: "success", message: `${options.done ?? `Done: ${bare(label)}`} (${seconds(elapsed)})` });
    }
  };
}

/**
 * Follow `work` as the operation `label` until it settles, and hand back its
 * result (or its rejection, unchanged). The way an extension names the
 * mutations its own client sends.
 */
export async function trackOperation<T>(
  label: string,
  work: Promise<T> | (() => Promise<T>),
  options: TrackOptions = {},
): Promise<T> {
  const end = beginOperation(label, options);
  try {
    const result = await (typeof work === "function" ? work() : work);
    end();
    return result;
  } catch (error) {
    end(error ?? new Error(`${bare(label)} failed`));
    throw error;
  }
}

/** The operations in flight, the hidden ones (younger than {@link SHOW_AFTER_MS}) included. */
export function useOperations(): Operation[] {
  return useOperationsStore((s) => s.operations);
}

/** Forget every operation (tests). */
export function resetOperations(): void {
  for (const t of timers.values()) clearTimeout(t);
  timers.clear();
  useOperationsStore.setState({ operations: [] });
}
