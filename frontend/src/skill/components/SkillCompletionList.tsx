import { useLayoutEffect, useRef, useState, type MouseEvent, type RefObject } from "react";
import { createPortal } from "react-dom";
import { CircleCheck, Sparkles } from "lucide-react";
import { caretOffset } from "../../shared/lib/caretOffset";
import { placeAtCaret, type CaretPlace } from "../../shared/lib/placeAtCaret";
import type { SkillOption } from "../model/skillCompletion";
import type { SkillCompletion } from "../store/useSkillPromptField";
import { appRoot, cssViewport, toCssRect } from "../../shared/ui/zoom";

const WIDTH = 330;
const MAX_HEIGHT = 280;

/** Who receives the skills: the words of the groups and of the details. */
export type SkillCompletionScope = "node" | "run";

/**
 * The skills `/` opens in a prompt, at the caret, in the look of the comment
 * editor's `CompletionList`: « On this node » (the skills it already
 * receives) then « Skill bank » (the folder as a hint, « adds it to the node »
 * as the detail), and the keys at the foot. A portal above everything, which
 * follows the page while it scrolls; it opens above the caret when the room
 * under it is short, and never leaves the viewport; its rows scroll to keep
 * the option chosen with the arrows in sight, with the head of its group
 * when it comes first under it. The pointer chooses a row only when it moves:
 * rows scrolling under a still pointer leave the choice to the arrows. A row
 * is picked with a click; the textarea keeps the focus.
 */
export default function SkillCompletionList({
  completion,
  textarea,
  onPick,
  scope,
}: {
  completion: SkillCompletion;
  textarea: RefObject<HTMLTextAreaElement | null>;
  onPick: (option: SkillOption) => void;
  scope: SkillCompletionScope;
}) {
  const { query, options, selected } = completion;
  const [place, setPlace] = useState<CaretPlace | null>(null);
  const rows = useRef<HTMLDivElement>(null);
  const foot = useRef<HTMLDivElement>(null);
  // Where the pointer last moved: a scroll fires the mouse events without moving it.
  const pointer = useRef<{ x: number; y: number } | null>(null);
  // What its height hangs on: the rows and the groups they fall in.
  const shown = options.map((option) => `${option.skill.id}${option.received ? "+" : ""}`).join(" ");

  useLayoutEffect(() => {
    const el = textarea.current;
    if (!query || !el) return;
    const measure = () => {
      // In the app's CSS pixels: it is drawn under the UI scale (`zoom.ts`).
      const box = toCssRect(el.getBoundingClientRect());
      const caret = caretOffset(el, query.start);
      const top = box.top + caret.top;
      // Its height unclamped: the rows' full height, the foot, the borders.
      const height = (rows.current?.scrollHeight ?? 0) + (foot.current?.offsetHeight ?? 0) + 2;
      setPlace(
        placeAtCaret(
          { top, bottom: top + caret.height, left: box.left + caret.left },
          { width: WIDTH, height, maxHeight: MAX_HEIGHT },
          cssViewport(),
        ),
      );
    };
    measure();
    window.addEventListener("scroll", measure, true);
    window.addEventListener("resize", measure);
    return () => {
      window.removeEventListener("scroll", measure, true);
      window.removeEventListener("resize", measure);
    };
  }, [query, shown, textarea]);

  // The option chosen with the arrows stays in sight while the rows scroll,
  // with the head of its group above it when it comes first in the group.
  useLayoutEffect(() => {
    const box = rows.current;
    const active = box?.querySelector<HTMLElement>('[aria-selected="true"]');
    if (!box || !active) return;
    if (selected === 0) {
      box.scrollTop = 0;
      return;
    }
    const before = active.previousElementSibling;
    const head = before && before.getAttribute("role") !== "option" ? before : active;
    const view = box.getBoundingClientRect();
    const top = head.getBoundingClientRect().top;
    const bottom = active.getBoundingClientRect().bottom;
    if (top < view.top) box.scrollTop -= view.top - top;
    else if (bottom > view.bottom) box.scrollTop += bottom - view.bottom;
  }, [selected, shown]);

  const hover = (index: number, event: MouseEvent) => {
    const last = pointer.current;
    pointer.current = { x: event.clientX, y: event.clientY };
    if (last && (last.x !== event.clientX || last.y !== event.clientY) && index !== selected) {
      completion.setSelected(index);
    }
  };

  if (!query || options.length === 0) return null;
  const received = options.filter((option) => option.received);
  const rest = options.filter((option) => !option.received);
  const row = (option: SkillOption, index: number) => {
    const active = index === selected;
    return (
      <div
        key={option.skill.id}
        role="option"
        aria-selected={active}
        data-testid={`skill-completion-option-${option.skill.name}`}
        data-received={option.received || undefined}
        onMouseDown={(event) => event.preventDefault()}
        onMouseMove={(event) => hover(index, event)}
        onClick={() => onPick(option)}
        className={`mx-1 grid cursor-pointer grid-cols-[auto_1fr_auto] items-center gap-2 rounded px-1.5 py-1 ${
          active ? "bg-selected" : "hover:bg-hover"
        }`}
      >
        <span className="inline-flex w-4 justify-center">
          {option.received ? (
            <CircleCheck size={11} className="text-st-await" />
          ) : (
            <Sparkles size={11} className="text-text-3" />
          )}
        </span>
        <span className="min-w-0 truncate">
          <span className="text-text">/{option.skill.name}</span>
          {option.folder && (
            <span className="font-mono text-text-3 text-2xs">
              {" "}
              {option.folder}
            </span>
          )}
        </span>
        <span className="text-text-3 text-2xs">
          {option.received ? `on this ${scope}` : `adds it to the ${scope}`}
        </span>
      </div>
    );
  };
  return createPortal(
    <div
      role="listbox"
      aria-label="Skills"
      data-testid="skill-completion-list"
      data-placement={place && "bottom" in place ? "above" : "below"}
      className="fixed z-[60] flex flex-col rounded-md border border-line-strong bg-surface-1 font-sans text-xs shadow-[0_12px_32px_var(--shadow)]"
      style={{
        ...(place ?? { top: 0, left: 0, maxHeight: MAX_HEIGHT }),
        width: WIDTH,
      }}
    >
      <div ref={rows} data-testid="skill-completion-rows" className="min-h-0 overflow-y-auto py-1">
        {received.length > 0 && <GroupHead label={`On this ${scope}`} />}
        {received.map((option, i) => row(option, i))}
        {rest.length > 0 && <GroupHead label="Skill bank" />}
        {rest.map((option, i) => row(option, received.length + i))}
      </div>
      <div ref={foot} className="border-t border-line px-2.5 py-1 text-text-3 text-2xs">
        ↑↓ choose · ↵ insert · esc close
      </div>
    </div>,
    appRoot() ?? document.body,
  );
}

function GroupHead({ label }: { label: string }) {
  return (
    <div className="px-2.5 pb-0.5 pt-1 uppercase tracking-wider text-text-3 text-2xs">
      {label}
    </div>
  );
}
