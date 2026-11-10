/**
 * How a host form lets its extension sections take part in its Save and
 * Cancel (`SectionForm`, ADR-0081 amendment #31): the sections register
 * handlers; the host runs them once it saved (with the saved entity, whose id
 * is known by then) or when it is cancelled. Since API 0.9 a section may also
 * provide a value the host submits under its extension's id (`provided`).
 *
 * Handlers run in the order of the sections on screen (registration order of
 * the extensions, then declaration order), whenever each section registered:
 * a section that re-registers on every keystroke keeps its place.
 */

import { useMemo, useRef } from "react";
import type { EntityContext, SectionForm } from "./contract";

type SaveHandler = (saved: EntityContext) => unknown;
type CancelHandler = () => unknown;
type ProvideHandler = () => unknown;

interface Ranked<H> {
  rank: number;
  seq: number;
  handler: H;
}

/** A provider, with the extension whose id its value goes under. */
interface Provider {
  extension: string;
  provide: ProvideHandler;
}

/** The host's side of `SectionForm`: a registration carries the section's rank. */
export interface RankedSectionForm extends SectionForm {
  onSave(handler: SaveHandler, rank?: number): () => void;
  onCancel(handler: CancelHandler, rank?: number): () => void;
  provide(handler: ProvideHandler, rank?: number, extension?: string): () => void;
}

export interface SectionFormHost {
  /** Handed to the host's `ExtensionSections`. Stable for the host's lifetime. */
  form: RankedSectionForm;
  /**
   * Run every section's save handler, in the sections' order, each awaited.
   * The first failure stops the run and rejects with its error: the host keeps
   * the form open and shows it. `saved` may be a loader, called only when a
   * section registered a handler (a host that must read the saved entity back,
   * e.g. for the Attributes the extensions stamped on it, costs nothing
   * without a section).
   */
  save(saved: EntityContext | (() => Promise<EntityContext>)): Promise<void>;
  /** Run every section's cancel handler; a failure is logged, never raised. */
  cancel(): void;
  /**
   * Ask every providing section for its value, in the sections' order, each
   * awaited: extension id → value, the objects of one extension merged. A
   * failure rejects with its error (the host keeps the form open). `{}`
   * without a provider.
   */
  provided(): Promise<Record<string, unknown>>;
}

let seq = 0;

function register<H>(list: Ranked<H>[], handler: H, rank = Number.MAX_SAFE_INTEGER): () => void {
  const entry = { rank, seq: seq++, handler };
  list.push(entry);
  return () => {
    const i = list.indexOf(entry);
    if (i >= 0) list.splice(i, 1);
  };
}

function inOrder<H>(list: Ranked<H>[]): H[] {
  return [...list].sort((a, b) => a.rank - b.rank || a.seq - b.seq).map((e) => e.handler);
}

/**
 * The form a section at `rank`, of the extension `extension`, receives: its
 * registrations carry its rank, its provided value goes under its extension.
 */
export function rankedForm(form: SectionForm, rank: number, extension: string): SectionForm {
  const host = form as RankedSectionForm;
  return {
    onSave: (handler) => host.onSave(handler, rank),
    onCancel: (handler) => host.onCancel(handler, rank),
    provide: (handler) => host.provide(handler, rank, extension),
  };
}

const isObject = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

/** Put `value` under `extension`, merged with an object it already provided. */
function merge(into: Record<string, unknown>, extension: string, value: unknown) {
  const before = into[extension];
  into[extension] = isObject(before) && isObject(value) ? { ...before, ...value } : value;
}

/** The Save and Cancel of one host form, for its sections. */
export function useSectionForm(): SectionFormHost {
  const saves = useRef<Ranked<SaveHandler>[]>([]);
  const cancels = useRef<Ranked<CancelHandler>[]>([]);
  const providers = useRef<Ranked<Provider>[]>([]);
  return useMemo(
    () => ({
      form: {
        onSave: (handler, rank) => register(saves.current, handler, rank),
        onCancel: (handler, rank) => register(cancels.current, handler, rank),
        provide: (provide, rank, extension = "") =>
          register(providers.current, { extension, provide }, rank),
      },
      provided: async () => {
        const values: Record<string, unknown> = {};
        for (const { extension, provide } of inOrder(providers.current)) {
          const value = await provide();
          if (value !== undefined && extension) merge(values, extension, value);
        }
        return values;
      },
      save: async (saved) => {
        const handlers = inOrder(saves.current);
        if (handlers.length === 0) return;
        const entity = typeof saved === "function" ? await saved() : saved;
        for (const handler of handlers) await handler(entity);
      },
      cancel: () => {
        for (const handler of inOrder(cancels.current)) {
          try {
            void Promise.resolve(handler()).catch((cause) =>
              console.warn("an extension section failed on cancel", cause),
            );
          } catch (cause) {
            console.warn("an extension section failed on cancel", cause);
          }
        }
      },
    }),
    [],
  );
}

/** The message a failed section save shows in its host. */
export function sectionSaveError(cause: unknown): string {
  const message = cause instanceof Error ? cause.message : String(cause);
  return `An extension section could not save: ${message}`;
}
