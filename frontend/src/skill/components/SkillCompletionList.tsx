import { useLayoutEffect, useState, type RefObject } from "react";
import { createPortal } from "react-dom";
import { CircleCheck, Sparkles } from "lucide-react";
import { caretOffset } from "../../shared/lib/caretOffset";
import type { SkillOption } from "../model/skillCompletion";
import type { SkillCompletion } from "../store/useSkillPromptField";

const WIDTH = 330;

/** Who receives the skills: the words of the groups and of the details. */
export type SkillCompletionScope = "node" | "run";

/**
 * The skills `/` opens in a prompt, at the caret, in the look of the comment
 * editor's `CompletionList`: « On this node » (the skills it already
 * receives) then « Skill bank » (the folder as a hint, « adds it to the node »
 * as the detail), and the keys at the foot. A portal above everything, which
 * follows the page while it scrolls. A row is picked with a click; the
 * textarea keeps the focus.
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
  const [place, setPlace] = useState<{ top: number; left: number } | null>(null);

  useLayoutEffect(() => {
    const el = textarea.current;
    if (!query || !el) return;
    const measure = () => {
      const box = el.getBoundingClientRect();
      const caret = caretOffset(el, query.start);
      const left = Math.max(4, Math.min(box.left + caret.left, window.innerWidth - WIDTH - 4));
      setPlace({ top: box.top + caret.top + caret.height + 2, left });
    };
    measure();
    window.addEventListener("scroll", measure, true);
    window.addEventListener("resize", measure);
    return () => {
      window.removeEventListener("scroll", measure, true);
      window.removeEventListener("resize", measure);
    };
  }, [query, textarea]);

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
        onMouseEnter={() => completion.setSelected(index)}
        onClick={() => onPick(option)}
        className={`mx-1 grid cursor-pointer grid-cols-[auto_1fr_auto] items-center gap-2 rounded px-1.5 py-1 ${
          active ? "bg-acc-bg" : "hover:bg-bg-3"
        }`}
      >
        <span className="inline-flex w-4 justify-center">
          {option.received ? (
            <CircleCheck size={11} className="text-st-await" />
          ) : (
            <Sparkles size={11} className="text-fg-4" />
          )}
        </span>
        <span className="min-w-0 truncate">
          <span className="text-fg">/{option.skill.name}</span>
          {option.folder && (
            <span className="font-mono text-fg-4" style={{ fontSize: "10px" }}>
              {" "}
              {option.folder}
            </span>
          )}
        </span>
        <span className="text-fg-4" style={{ fontSize: "10px" }}>
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
      className="fixed z-[60] flex max-h-[280px] flex-col rounded-md border border-line-strong bg-bg-2 font-sans shadow-[0_12px_32px_rgba(0,0,0,0.55)]"
      style={{ top: place?.top ?? 0, left: place?.left ?? 0, width: WIDTH, fontSize: "11px" }}
    >
      <div className="min-h-0 overflow-y-auto py-1">
        {received.length > 0 && <GroupHead label={`On this ${scope}`} />}
        {received.map((option, i) => row(option, i))}
        {rest.length > 0 && <GroupHead label="Skill bank" />}
        {rest.map((option, i) => row(option, received.length + i))}
      </div>
      <div className="border-t border-line px-2.5 py-1 text-fg-4" style={{ fontSize: "9.5px" }}>
        ↑↓ choose · ↵ insert · esc close
      </div>
    </div>,
    document.body,
  );
}

function GroupHead({ label }: { label: string }) {
  return (
    <div className="px-2.5 pb-0.5 pt-1 uppercase tracking-wider text-fg-4" style={{ fontSize: "9px" }}>
      {label}
    </div>
  );
}
