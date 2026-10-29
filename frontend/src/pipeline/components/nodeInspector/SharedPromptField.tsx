import { useLayoutEffect, useRef, useState, type RefObject, type TextareaHTMLAttributes } from "react";
import type { DraftParticipant } from "../../../shared/generated/DraftParticipant";
import { shiftCaret } from "../../model/draftDelta";
import { firstName, presenceStyle } from "../../model/presence";
import { draftSessionOf } from "../../store/draftSessions";
import { useEditStore } from "../../store/editStore";

/** The pixel position of `caret` inside `textarea`, by a mirror of its text. */
function caretPosition(textarea: HTMLTextAreaElement, caret: number): { top: number; left: number } {
  const mirror = document.createElement("div");
  const style = window.getComputedStyle(textarea);
  for (const property of [
    "boxSizing", "width", "paddingTop", "paddingRight", "paddingBottom", "paddingLeft",
    "borderTopWidth", "borderRightWidth", "borderBottomWidth", "borderLeftWidth",
    "fontFamily", "fontSize", "fontWeight", "lineHeight", "letterSpacing", "tabSize",
  ] as const) {
    mirror.style[property] = style[property];
  }
  mirror.style.position = "absolute";
  mirror.style.visibility = "hidden";
  mirror.style.whiteSpace = "pre-wrap";
  mirror.style.overflowWrap = "break-word";
  mirror.textContent = textarea.value.slice(0, caret);
  const marker = document.createElement("span");
  marker.textContent = "​";
  mirror.appendChild(marker);
  document.body.appendChild(mirror);
  const position = { top: marker.offsetTop - textarea.scrollTop, left: marker.offsetLeft - textarea.scrollLeft };
  mirror.remove();
  return position;
}

/**
 * A node's prompt in its shared draft (ADR-0088): the text others type lands
 * without moving my caret, and the caret of each other participant typing here
 * shows in their color, with their first name. The textarea's own handlers
 * (a completion's keys, its `ref`) run beside the draft's.
 */
export default function SharedPromptField({
  nodeId,
  value,
  onChange,
  onSelect,
  onFocus,
  onBlur,
  typing,
  ref: outerRef,
  ...rest
}: Omit<TextareaHTMLAttributes<HTMLTextAreaElement>, "value"> & {
  nodeId: string;
  value: string;
  typing: DraftParticipant[];
  ref?: RefObject<HTMLTextAreaElement | null>;
}) {
  const ref = useRef<HTMLTextAreaElement | null>(null);
  const shown = useRef(value);
  const [carets, setCarets] = useState<{ participant: DraftParticipant; top: number; left: number }[]>([]);

  // A change from elsewhere: my caret keeps its place in the text.
  useLayoutEffect(() => {
    const textarea = ref.current;
    const before = shown.current;
    shown.current = value;
    if (!textarea || before === value || document.activeElement !== textarea) return;
    const start = shiftCaret(textarea.selectionStart, before, value);
    const end = shiftCaret(textarea.selectionEnd, before, value);
    textarea.setSelectionRange(start, end);
  }, [value]);

  useLayoutEffect(() => {
    const textarea = ref.current;
    if (!textarea || typing.length === 0) {
      setCarets([]);
      return;
    }
    setCarets(
      typing.map((participant) => ({
        participant,
        ...caretPosition(textarea, Math.min(participant.typing?.caret ?? 0, value.length)),
      })),
    );
  }, [typing, value]);

  const report = () => {
    const textarea = ref.current;
    if (!textarea) return;
    const session = draftSessionOf(useEditStore.getState().activeTabId);
    session?.present({ typing: { node_id: nodeId, field: "prompt", caret: textarea.selectionStart } });
  };

  return (
    <div className="ei-prompt">
      <textarea
        {...rest}
        ref={(textarea) => {
          ref.current = textarea;
          if (outerRef) outerRef.current = textarea;
        }}
        value={value}
        onChange={(e) => {
          shown.current = e.target.value;
          onChange?.(e);
          report();
        }}
        onSelect={(e) => {
          onSelect?.(e);
          report();
        }}
        onFocus={(e) => {
          onFocus?.(e);
          draftSessionOf(useEditStore.getState().activeTabId)?.openPrompt(nodeId);
          report();
        }}
        onBlur={(e) => {
          onBlur?.(e);
          draftSessionOf(useEditStore.getState().activeTabId)?.present({ typing: null });
        }}
      />
      {carets.map(({ participant, top, left }) => (
        <span
          key={participant.connection}
          className="ei-caret"
          style={{ top, left, ...presenceStyle(participant) }}
          data-testid="remote-caret"
        >
          <span className="ei-caret__bar" />
          <span className="ei-caret__name">{firstName(participant)}</span>
        </span>
      ))}
    </div>
  );
}
