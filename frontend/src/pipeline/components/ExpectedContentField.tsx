import { useEffect, useRef, useState } from "react";
import { ChevronRight, FileText } from "lucide-react";

/**
 * « Expected content »: what the agent is told the file holds, folded to a
 * one-line preview until clicked. Shared by an output and a live artifact.
 */
export default function ExpectedContentField({
  value,
  onChange,
}: {
  value: string | null | undefined;
  onChange: (value: string | undefined) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const instructions = value?.trim() ? value : "";
  const preview = instructions.replace(/\s+/g, " ").trim();

  useEffect(() => {
    if (!expanded || !textareaRef.current) return;
    const textarea = textareaRef.current;
    textarea.focus();
    textarea.style.height = "auto";
    const height = Math.min(textarea.scrollHeight, 150);
    textarea.style.height = `${height}px`;
    textarea.style.overflowY = textarea.scrollHeight > 150 ? "auto" : "hidden";
  }, [instructions, expanded]);

  useEffect(() => {
    if (!expanded) return;
    const collapseOnOutsideClick = (event: MouseEvent) => {
      // A press that keeps the focus where it is — a tour card's buttons
      // prevent its default — keeps the field open. Folding under it moved the
      // card up before the release, and the click missed Next (FP of CORE-227).
      if (event.defaultPrevented) return;
      if (!containerRef.current?.contains(event.target as Node)) {
        setExpanded(false);
      }
    };
    document.addEventListener("mousedown", collapseOnOutsideClick);
    return () => document.removeEventListener("mousedown", collapseOnOutsideClick);
  }, [expanded]);

  return (
    <div ref={containerRef} className="ei-exp" data-open={expanded || undefined}>
      <button
        type="button"
        aria-label={instructions ? "Edit expected content" : "Add expected content"}
        aria-expanded={expanded}
        title="Guides the agent, never checked"
        className="ei-exp__trigger"
        onClick={() => setExpanded((open) => !open)}
      >
        <FileText aria-hidden="true" size={12} strokeWidth={1.5} />
        <span className="ei-exp__label">Expected content</span>
        <span className="ei-exp__preview">{preview}</span>
        <ChevronRight aria-hidden="true" className="ei-exp__chev" size={12} strokeWidth={1.5} />
      </button>
      {expanded && (
        <textarea
          ref={textareaRef}
          aria-label="Expected content"
          title="Esc folds"
          className="d-input d-area"
          value={value ?? ""}
          onChange={(event) => {
            const next = event.target.value;
            onChange(next.trim() ? next : undefined);
          }}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.stopPropagation();
              setExpanded(false);
            }
          }}
        />
      )}
    </div>
  );
}
