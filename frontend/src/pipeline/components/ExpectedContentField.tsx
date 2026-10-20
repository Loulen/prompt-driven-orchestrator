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
      if (!containerRef.current?.contains(event.target as Node)) {
        setExpanded(false);
      }
    };
    document.addEventListener("mousedown", collapseOnOutsideClick);
    return () => document.removeEventListener("mousedown", collapseOnOutsideClick);
  }, [expanded]);

  return (
    <div ref={containerRef} className={`exp${expanded ? " expanded" : ""}`}>
      <button
        type="button"
        aria-label={instructions ? "Edit expected content" : "Add expected content"}
        aria-expanded={expanded}
        className="exp-trigger"
        onClick={() => setExpanded((open) => !open)}
      >
        <FileText aria-hidden="true" size={12} />
        <span className="exp-label">Expected content</span>
        <span className={`exp-preview${preview ? "" : " empty"}`}>{preview || "+ add"}</span>
        <ChevronRight aria-hidden="true" className="exp-chevron" size={12} />
      </button>
      {expanded && (
        <>
          <textarea
            ref={textareaRef}
            aria-label="Expected content"
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
          <div className="exp-footer">
            <span>guide l'agent · non vérifié</span>
            <span>échap replie</span>
          </div>
        </>
      )}
    </div>
  );
}
