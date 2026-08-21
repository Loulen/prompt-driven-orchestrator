import type { MouseEvent } from "react";
import { Check } from "lucide-react";

interface Props {
  /** Whether this row is checked (in the selection set). */
  selected: boolean;
  /**
   * The resting status-dot colour (`bg-st-*`). Omit/null for a row with no
   * status (a Library pipeline) — the slot is empty at rest and only the hover
   * ring fades in, keeping the leading slot identical in shape across tabs.
   */
  dotClass?: string | null;
  /**
   * The hover-ring colour (`border-st-*`) — the filled dot "goes hollow" in its
   * own colour on hover. Defaults to a neutral ring (used by rows with no status
   * colour). Only meaningful while unselected.
   */
  ringClass?: string;
  /** Pulse the resting dot (a live/running row). */
  pulse?: boolean;
  /** Native tooltip on the resting dot (e.g. a run's failure reason). */
  dotTitle?: string;
  /** Accessible name + tooltip for the control itself. */
  label: string;
  onSelect: (e: MouseEvent) => void;
  /** Forwarded to the RESTING dot so existing dot assertions keep working
   *  (`run-status-dot` / `trigger-status-dot`). */
  dotTestId?: string;
  /** data-testid on the interactive control (the select hit-target). */
  testId?: string;
}

/**
 * The leading select control shared by every left-panel row (#577, design "D").
 * The status dot *is* the checkbox: at rest it shows the row's status colour; on
 * row hover it morphs to a hollow ring ("the inside goes empty"); clicking it
 * selects the row and it becomes a green check. Clicking the control selects
 * (and stops propagation so the row body's open/navigate is untouched); a
 * shift-click is forwarded via the event so the caller can extend a range.
 *
 * Exactly one of {resting dot, hover ring} is `display`ed at any time, so the
 * single-cell grid always centres the visible glyph — no layout shift between
 * states. The resting dot stays in the DOM even while hover-hidden, which is why
 * status-colour and failure-tooltip assertions on it survive this refactor.
 */
export default function SelectControl({
  selected,
  dotClass,
  ringClass = "border-fg-3",
  pulse = false,
  dotTitle,
  label,
  onSelect,
  dotTestId,
  testId,
}: Props) {
  return (
    <span
      role="checkbox"
      aria-checked={selected}
      aria-label={label}
      title={label}
      data-testid={testId}
      onClick={(e) => {
        e.stopPropagation();
        onSelect(e);
      }}
      className="grid h-4 w-4 shrink-0 cursor-pointer place-items-center"
    >
      {selected ? (
        <span
          className="grid h-4 w-4 place-items-center rounded-full bg-acc text-[#04140d]"
          data-testid="select-check"
        >
          <Check size={11} strokeWidth={3} />
        </span>
      ) : (
        <>
          {/* Resting status dot — hidden (but kept in the DOM) on row hover. */}
          <span
            data-testid={dotTestId}
            title={dotTitle}
            className={`block h-2 w-2 rounded-full group-hover:hidden ${
              dotClass ?? "opacity-0"
            } ${pulse ? "animate-pulse" : ""}`}
          />
          {/* Hover ring — the filled dot gone hollow; only shown on row hover. */}
          <span
            aria-hidden="true"
            className={`hidden h-3.5 w-3.5 rounded-full border-2 group-hover:block ${ringClass}`}
          />
        </>
      )}
    </span>
  );
}
