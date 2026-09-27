import { useRef } from "react";
import { FolderOpen, Square } from "lucide-react";

/** The overlay itself (design 02): one line for the count, one for the two rules. */
export function DropOverlay({ count, hint }: { count: number; hint: string }) {
  return (
    <div
      className="pointer-events-none absolute inset-0 z-20 flex items-center justify-center rounded-lg border-2 border-dashed border-acc/70 bg-bg-1/70"
      data-testid="skill-drop-overlay"
      role="status"
    >
      <div className="flex flex-col gap-2 rounded-lg border border-line bg-bg-4 p-3 shadow-xl">
        <div className="rounded-md border border-line-strong bg-bg-3 px-8 py-3 text-center font-semibold text-fg" style={{ fontSize: "14px" }}>
          Drop to attach {count} file{count === 1 ? "" : "s"}
        </div>
        <div className="rounded-md border border-acc/40 bg-bg-3 px-4 py-2.5 text-center text-fg-3" style={{ fontSize: "11px" }}>
          {hint}
        </div>
      </div>
    </div>
  );
}

interface Props {
  /** The bar's text after the leading label (`Files · 2 · drop more anywhere, or`). */
  label: React.ReactNode;
  /** Browse… : open the explorer. */
  onBrowse: () => void;
  /** Files chosen in the native picker (`<input type=file multiple>`). */
  onPickFiles: (files: FileList) => void;
  disabled?: boolean;
  compact?: boolean;
  testId?: string;
}

/**
 * The drop bar (#671 design 01/03/05): a focusable row with the checkbox-like
 * glyph, the sentence, and Browse…. Keyboard: Enter/Space on the bar opens the
 * native file picker (multi-select, files only); Browse… opens the daemon-side
 * explorer. The surface that accepts the drop is the parent (`useFileDropTarget`).
 */
export default function SkillFileDropZone({ label, onBrowse, onPickFiles, disabled, compact, testId = "skill-drop-zone" }: Props) {
  const inputRef = useRef<HTMLInputElement>(null);
  const openPicker = () => {
    if (disabled) return;
    inputRef.current?.click();
  };
  return (
    <div
      role="button"
      tabIndex={disabled ? -1 : 0}
      aria-disabled={disabled || undefined}
      aria-label="Add reference files"
      data-testid={testId}
      onClick={openPicker}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          openPicker();
        }
      }}
      className={`flex items-center gap-2 rounded-md border border-dashed border-line-strong bg-bg-3 text-fg-3 outline-none transition-colors focus-visible:border-acc hover:border-acc/60 ${
        compact ? "px-2.5 py-1.5" : "px-3 py-2"
      } ${disabled ? "opacity-50" : "cursor-pointer"}`}
      style={{ fontSize: "10.5px" }}
    >
      <Square size={11} className="shrink-0 text-fg-4" aria-hidden />
      <span className="min-w-0 flex-1 truncate">{label}</span>
      <button
        type="button"
        disabled={disabled}
        data-testid={`${testId}-browse`}
        onClick={(event) => {
          event.stopPropagation();
          onBrowse();
        }}
        className="flex shrink-0 items-center gap-1 rounded-md border border-line-strong bg-bg-4 px-2 py-1 text-fg-2 hover:border-acc disabled:opacity-50"
        style={{ fontSize: "10.5px" }}
      >
        <FolderOpen size={10} />
        Browse…
      </button>
      <input
        ref={inputRef}
        type="file"
        multiple
        hidden
        data-testid={`${testId}-input`}
        // The programmatic `click()` bubbles back to the bar's onClick: stop it here.
        onClick={(event) => event.stopPropagation()}
        onChange={(event) => {
          if (event.target.files && event.target.files.length > 0) onPickFiles(event.target.files);
          event.target.value = "";
        }}
      />
    </div>
  );
}
