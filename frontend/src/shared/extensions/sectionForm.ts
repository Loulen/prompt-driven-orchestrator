/**
 * How a host form lets its extension sections take part in its Save and
 * Cancel (`SectionForm`, ADR-0081 amendment #31): the sections register
 * handlers; the host runs them once it saved (with the saved entity, whose id
 * is known by then) or when it is cancelled.
 *
 * Handlers run in the order of the sections on screen (registration order of
 * the extensions, then declaration order), whenever each section registered:
 * a section that re-registers on every keystroke keeps its place.
 */

import { useMemo, useRef } from "react";
import type { EntityContext, SectionForm } from "./contract";

type SaveHandler = (saved: EntityContext) => unknown;
type CancelHandler = () => unknown;

interface Ranked<H> {
  rank: number;
  seq: number;
  handler: H;
}

/** The host's side of `SectionForm`: a registration carries the section's rank. */
export interface RankedSectionForm extends SectionForm {
  onSave(handler: SaveHandler, rank?: number): () => void;
  onCancel(handler: CancelHandler, rank?: number): () => void;
}

export interface SectionFormHost {
  /** Handed to the host's `ExtensionSections`. Stable for the host's lifetime. */
  form: RankedSectionForm;
  /**
   * Run every section's save handler, in the sections' order, each awaited.
   * The first failure stops the run and rejects with its error: the host keeps
   * the form open and shows it.
   */
  save(saved: EntityContext): Promise<void>;
  /** Run every section's cancel handler; a failure is logged, never raised. */
  cancel(): void;
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

/** The form a section at `rank` receives: its registrations carry its rank. */
export function rankedForm(form: SectionForm, rank: number): SectionForm {
  const host = form as RankedSectionForm;
  return {
    onSave: (handler) => host.onSave(handler, rank),
    onCancel: (handler) => host.onCancel(handler, rank),
  };
}

/** The Save and Cancel of one host form, for its sections. */
export function useSectionForm(): SectionFormHost {
  const saves = useRef<Ranked<SaveHandler>[]>([]);
  const cancels = useRef<Ranked<CancelHandler>[]>([]);
  return useMemo(
    () => ({
      form: {
        onSave: (handler, rank) => register(saves.current, handler, rank),
        onCancel: (handler, rank) => register(cancels.current, handler, rank),
      },
      save: async (saved) => {
        for (const handler of inOrder(saves.current)) await handler(saved);
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
