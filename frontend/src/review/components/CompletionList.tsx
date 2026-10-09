import { useLayoutEffect, useState, type RefObject } from "react";
import { createPortal } from "react-dom";
import { ActorAvatar } from "../../shared/actors/ActorAvatar";
import type { Completion, CompletionItem } from "../../shared/extensions/useCompletion";
import { caretOffset } from "../../shared/lib/caretOffset";

/**
 * The suggestions of the comment editor's sources of completion (API 0.14),
 * in the style of the `RefPicker` popover, without a header nor a keyboard
 * hint: avatar, label, a monospace hint, a detail on the right. It opens under
 * the trigger, above everything (a portal: the diff's boxes clip), and follows
 * the page while it scrolls. A row is picked with a click; the textarea keeps
 * the focus.
 */

const WIDTH = 320;

interface Props {
  completion: Completion;
  textarea: RefObject<HTMLTextAreaElement | null>;
  onPick: (item: CompletionItem) => void;
}

export default function CompletionList({ completion, textarea, onPick }: Props) {
  const { query, items, selected } = completion;
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

  if (!query || items.length === 0) return null;
  // The avatar column only when a suggestion has one.
  const avatars = items.some((item) => item.suggestion.avatar);
  return createPortal(
    <div
      role="listbox"
      data-testid="review-completion-list"
      className="fixed z-[60] max-h-[240px] overflow-y-auto rounded-md border border-line-strong bg-bg-2 py-1 font-sans shadow-[0_12px_32px_rgba(0,0,0,0.55)]"
      style={{ top: place?.top ?? 0, left: place?.left ?? 0, width: WIDTH, fontSize: "11px" }}
    >
      {items.map((item, i) => {
        const s = item.suggestion;
        const active = i === selected;
        return (
          <div
            key={item.key}
            role="option"
            aria-selected={active}
            data-testid="review-completion-option"
            onMouseDown={(e) => e.preventDefault()}
            onMouseEnter={() => completion.setSelected(i)}
            onClick={() => onPick(item)}
            className={`mx-1 grid cursor-pointer ${avatars ? "grid-cols-[auto_1fr_auto]" : "grid-cols-[1fr_auto]"} items-center gap-2 rounded px-1.5 py-1 ${
              active ? "bg-acc-bg" : "hover:bg-bg-3"
            }`}
          >
            {avatars && (
              <span className="inline-flex w-4 justify-center">
                {s.avatar && <ActorAvatar display={{ name: s.avatar.name, avatar_url: s.avatar.avatar_url ?? null }} size={16} />}
              </span>
            )}
            <span className="min-w-0 truncate">
              <span className="text-fg">{s.label}</span>
              {s.hint && (
                <span className="font-mono text-fg-4" style={{ fontSize: "10px" }}>
                  {" "}
                  {s.hint}
                </span>
              )}
            </span>
            <span className="text-fg-4" style={{ fontSize: "10px" }}>
              {s.detail ?? ""}
            </span>
          </div>
        );
      })}
    </div>,
    document.body,
  );
}
