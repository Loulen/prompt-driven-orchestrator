import type { MouseEvent } from "react";
import { Check } from "lucide-react";
import StatusIcon from "./StatusIcon";
import { statusTitle, type StatusKey } from "../lib/statusTable";

interface Props {
  /** Whether this row is checked (in the selection set). */
  selected: boolean;
  /**
   * The row's status, shown at rest as its Status icon. Omit/null for a row with
   * no status (a Library pipeline, a group header) — the slot is empty at rest
   * and only the hover square fades in, keeping the leading slot identical in
   * shape across tabs.
   */
  status?: StatusKey | null;
  /** Added to the status tooltip (e.g. a run's failure or awaiting reason). */
  statusReason?: string | null;
  /**
   * The list is in selection mode (at least one row checked): every control
   * stays a square until the selection is cleared, and the row's status moves
   * into the control's tooltip.
   */
  selecting?: boolean;
  /** Accessible name + tooltip for the control itself. */
  label: string;
  onSelect: (e: MouseEvent) => void;
  /** Forwarded to the RESTING Status icon so existing dot assertions keep
   *  working (`run-status-dot` / `trigger-status-dot`). */
  dotTestId?: string;
  /** data-testid on the interactive control (the select hit-target). */
  testId?: string;
}

/**
 * The leading select control shared by every left-panel row (#577, PDO-22 #997).
 * The round is reserved for the status, the square for the selection: at rest
 * the slot shows the row's Status icon; on row hover it gives way to an empty
 * square; once checked it is a filled accent square with a check — never a
 * round check that could pass for « completed ». While the list is in selection
 * mode, every unchecked row shows the empty square too.
 *
 * The Status icon is a SIBLING of the checkbox (a checkbox's children are
 * presentational), so a screen reader still announces it; it stays in the DOM
 * (visually hidden) while the square shows. The checkbox overlays the whole
 * cell, so clicking the glyph selects the row; its tooltip carries the status
 * (« Select x · Failed: reason ») since hovering the row hides the glyph.
 */
export default function SelectControl({
  selected,
  status,
  statusReason,
  selecting = false,
  label,
  onSelect,
  dotTestId,
  testId,
}: Props) {
  const squareMode = selected || selecting;
  const title = status ? `${label} · ${statusTitle(status, statusReason)}` : label;
  return (
    <span className="relative grid h-4 w-4 shrink-0 place-items-center">
      {status && (
        <StatusIcon
          status={status}
          reason={statusReason}
          testId={dotTestId}
          className={squareMode ? "sr-only" : "group-hover:hidden"}
        />
      )}
      <span
        role="checkbox"
        aria-checked={selected}
        aria-label={label}
        title={title}
        data-testid={testId}
        data-select-mode={squareMode ? "square" : "glyph"}
        onClick={(e) => {
          e.stopPropagation();
          onSelect(e);
        }}
        className="absolute inset-0 grid cursor-pointer place-items-center"
      >
        {selected ? (
          <span
            className="grid h-3 w-3 place-items-center rounded-[2.5px] bg-acc text-on-acc"
            data-testid="select-check"
          >
            <Check size={9} strokeWidth={3.5} />
          </span>
        ) : (
          <span
            aria-hidden="true"
            data-testid="select-square"
            className={`h-3 w-3 rounded-[2.5px] border-[1.5px] border-fg-3 ${
              selecting ? "block" : "hidden group-hover:block"
            }`}
          />
        )}
      </span>
    </span>
  );
}
