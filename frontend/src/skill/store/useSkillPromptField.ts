import { useCallback, useMemo, useRef, useState } from "react";
import type { Skill } from "../../shared/generated/Skill";
import type { SkillFolder } from "../../shared/generated/SkillFolder";
import { insertSuggestion, type ActiveQuery } from "../../shared/extensions/completion";
import { SKILL_TRIGGER, skillOptions, skillQuery, type SkillOption } from "../model/skillCompletion";

/** The open list: where it was opened, its rows, the row the keyboard points at. */
export interface SkillCompletion {
  query: ActiveQuery | null;
  options: SkillOption[];
  selected: number;
  setSelected: (index: number) => void;
}

interface Args {
  value: string;
  onChange: (value: string) => void;
  bank: { skills: Skill[]; folders: SkillFolder[] };
  /** The ids the node (or the Run) already receives, from every tier. */
  received: ReadonlySet<string>;
  /** A picked skill it does not receive yet; absent where nothing can be added. */
  onAdd?: (skill: Skill) => void;
}

/**
 * A prompt textarea that completes skills: `/` at the start of a word opens the
 * bank's skills at the caret (received first), ↑↓ move, Enter or Tab insert
 * `/<name>`, Escape closes without inserting. The keys are the extension
 * sources' (`useCompletion`): the list takes them only while it is open.
 *
 * Spread `fieldProps` on the textarea and render `SkillCompletionList` with
 * `completion` and `pick`.
 */
export function useSkillPromptField({ value, onChange, bank, received, onAdd }: Args) {
  const ref = useRef<HTMLTextAreaElement | null>(null);
  const [query, setQuery] = useState<ActiveQuery | null>(null);
  const [selected, setSelected] = useState(0);
  const [dismissedAt, setDismissedAt] = useState<number | null>(null);
  /** The query of the last update, read synchronously by the next one. */
  const current = useRef<ActiveQuery | null>(null);

  const options = useMemo(
    () => (query ? skillOptions(bank, received, query.query) : []),
    [query, bank, received],
  );
  const open = query !== null && options.length > 0 && dismissedAt !== query.start;

  const update = useCallback((text: string, caret: number) => {
    const next = skillQuery(text, caret);
    const former = current.current;
    if (!former || !next || former.start !== next.start || former.query !== next.query) setSelected(0);
    current.current = next;
    setQuery(next);
    setDismissedAt((at) => (next && at === next.start ? at : null));
  }, []);

  const close = useCallback(() => {
    current.current = null;
    setQuery(null);
  }, []);

  const pick = useCallback(
    (option: SkillOption) => {
      if (!query) return;
      const done = insertSuggestion(value, query, `${SKILL_TRIGGER}${option.skill.name}`);
      onChange(done.text);
      if (!option.received) onAdd?.(option.skill);
      close();
      window.setTimeout(() => {
        ref.current?.focus();
        ref.current?.setSelectionRange(done.caret, done.caret);
      }, 0);
    },
    [query, value, onChange, onAdd, close],
  );

  const onKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (!open || event.nativeEvent.isComposing) return;
    const n = options.length;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      const step = event.key === "ArrowDown" ? 1 : -1;
      setSelected((i) => (i + step + n) % n);
    } else if ((event.key === "Enter" || event.key === "Tab") && !event.metaKey && !event.ctrlKey) {
      pick(options[Math.min(selected, n - 1)]);
    } else if (event.key === "Escape") {
      setDismissedAt(query.start);
    } else {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
  };

  const fieldProps = {
    ref,
    value,
    onChange: (event: React.ChangeEvent<HTMLTextAreaElement>) => {
      onChange(event.target.value);
      update(event.target.value, event.target.selectionStart ?? event.target.value.length);
    },
    onClick: (event: React.MouseEvent<HTMLTextAreaElement>) =>
      update(event.currentTarget.value, event.currentTarget.selectionStart ?? 0),
    onKeyUp: (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
      // The caret moved without a change: the list follows it.
      if (["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) {
        update(event.currentTarget.value, event.currentTarget.selectionStart ?? 0);
      }
    },
    onKeyDown,
    onBlur: close,
  };

  const completion: SkillCompletion = {
    query: open ? query : null,
    options: open ? options : [],
    selected: Math.min(selected, Math.max(options.length - 1, 0)),
    setSelected,
  };

  return { fieldProps, completion, pick };
}
