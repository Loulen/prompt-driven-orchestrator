/**
 * A text field's sources of completion (API 0.14): the list the trigger
 * opens, the suggestions of every source of that trigger in contribution
 * order, the row the keyboard points at, and the pick. Without a source, the
 * field behaves as without extension: nothing opens, no key is taken.
 */

import { useCallback, useMemo, useRef, useState } from "react";
import type { UiLocation } from "../generated/UiLocation";
import { useContributions, type ContributionElement } from "./catalogue";
import { activeQuery, insertSuggestion, type ActiveQuery } from "./completion";
import type { CompletionSuggestion, ReviewComposerContext } from "./contract";

export interface CompletionItem {
  /** Unique in the list: `<extension>:<contribution>:<suggestion id>`. */
  key: string;
  element: ContributionElement;
  suggestion: CompletionSuggestion;
}

export interface Completion {
  /** The query the list is open on; `null` while closed. */
  query: ActiveQuery | null;
  items: CompletionItem[];
  /** The row Enter or Tab would pick. */
  selected: number;
  setSelected: (index: number) => void;
  /** Read the text and the caret after a change, or a caret move. */
  update: (text: string, caret: number) => void;
  /** The text once `item` is picked, and the caret after the insertion. */
  pick: (text: string, item: CompletionItem) => { text: string; caret: number } | null;
  /** Close the list until the query changes place (Escape). */
  dismiss: () => void;
  /** Close the list; the next keystroke may open it again (the field lost the focus). */
  close: () => void;
  /** Handle a key of the field: the arrows, Enter and Tab pick, Escape closes.
   *  `true` when the list took it. */
  onKeyDown: (event: React.KeyboardEvent, onPick: (item: CompletionItem) => void) => boolean;
}

function readSuggestions(answer: unknown): CompletionSuggestion[] {
  if (!Array.isArray(answer)) return [];
  return answer.filter(
    (s): s is CompletionSuggestion =>
      !!s && typeof s === "object" && typeof s.label === "string" && typeof s.insert === "string",
  );
}

export function useCompletion(location: UiLocation, context: ReviewComposerContext): Completion {
  const all = useContributions(location, context);
  const sources = useMemo(() => all.filter((e) => e.completion), [all]);
  const triggers = useMemo(() => [...new Set(sources.map((e) => e.completion!.trigger))], [sources]);
  const [query, setQuery] = useState<ActiveQuery | null>(null);
  const [items, setItems] = useState<CompletionItem[]>([]);
  const [selected, setSelected] = useState(0);
  const [dismissedAt, setDismissedAt] = useState<number | null>(null);
  const asked = useRef(0);
  /** The query of the last update, read synchronously by the next one. */
  const current = useRef<ActiveQuery | null>(null);

  const update = useCallback(
    (text: string, caret: number) => {
      const next = triggers.length > 0 ? activeQuery(text, caret, triggers) : null;
      const ask = ++asked.current;
      if (!next) {
        current.current = null;
        setQuery(null);
        setItems([]);
        setDismissedAt(null);
        return;
      }
      setDismissedAt((at) => (at === next.start ? at : null));
      // Another trigger, or another word: the former suggestions are not this query's.
      const former = current.current;
      if (!former || former.trigger !== next.trigger || former.start !== next.start) setItems([]);
      current.current = next;
      setQuery(next);
      const of = sources.filter((e) => e.completion!.trigger === next.trigger);
      void Promise.all(
        of.map(async (element) => {
          try {
            const answer = await element.completion!.suggest(next.query, context);
            return readSuggestions(answer).map((suggestion) => ({
              key: `${element.key}:${suggestion.id}`,
              element,
              suggestion,
            }));
          } catch (cause) {
            console.warn(`extension ${element.extension.id}: the source ${element.contribution.id} failed`, cause);
            return [];
          }
        }),
      ).then((lists) => {
        // A slower answer to an older query never replaces a newer one.
        if (ask !== asked.current) return;
        setItems(lists.flat());
        setSelected(0);
      });
    },
    [sources, triggers, context],
  );

  const open = query !== null && items.length > 0 && dismissedAt !== query.start;

  const dismiss = useCallback(() => setDismissedAt(query?.start ?? null), [query]);

  const close = useCallback(() => {
    asked.current += 1;
    current.current = null;
    setQuery(null);
    setItems([]);
  }, []);

  const pick = useCallback(
    (text: string, item: CompletionItem) => {
      if (!query) return null;
      const done = insertSuggestion(text, query, item.suggestion.insert);
      asked.current += 1;
      current.current = null;
      setQuery(null);
      setItems([]);
      return done;
    },
    [query],
  );

  const onKeyDown = (event: React.KeyboardEvent, onPick: (item: CompletionItem) => void): boolean => {
    if (!open || event.nativeEvent.isComposing) return false;
    const n = items.length;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      const step = event.key === "ArrowDown" ? 1 : -1;
      setSelected((i) => (i + step + n) % n);
    } else if ((event.key === "Enter" || event.key === "Tab") && !event.metaKey && !event.ctrlKey) {
      onPick(items[Math.min(selected, n - 1)]);
    } else if (event.key === "Escape") {
      dismiss();
    } else {
      return false;
    }
    event.preventDefault();
    event.stopPropagation();
    return true;
  };

  return {
    query: open ? query : null,
    items: open ? items : [],
    selected,
    setSelected,
    update,
    pick,
    dismiss,
    close,
    onKeyDown,
  };
}
