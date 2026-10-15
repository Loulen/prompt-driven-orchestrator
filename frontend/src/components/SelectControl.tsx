import type { MouseEvent } from "react";
import { Check } from "lucide-react";
import StatusIcon from "./StatusIcon";
import { statusSpec, statusTitle, type StatusKey } from "../lib/statusTable";

interface Props {
  /** Whether this row is checked (in the selection set). */
  selected: boolean;
  /**
   * The row's status, shown at rest as its Status icon and lending its colour to
   * the hover ring. Omit/null for a row with no status (a Library pipeline, a
   * group header) — the slot is empty at rest and only a neutral ring fades in,
   * keeping the leading slot identical in shape across tabs.
   */
  status?: StatusKey | null;
  /** Added to the status tooltip (e.g. a run's failure or awaiting reason). */
  statusReason?: string | null;
  /**
   * The list is in selection mode (at least one row checked): every unchecked
   * control stays a ring in its status colour until the selection is cleared,
   * and the row's status moves into the control's tooltip.
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
 * The leading select control shared by every left-panel row (#577, PDO-22 #997,
 * round again since #1005 — the 1.119.1 shape). At rest the slot shows the row's
 * Status icon; on row hover it gives way to a hollow 14 px ring in the status
 * colour (neutral `fg-3` for a row with no status), so the status stays readable
 * by its colour; once checked it is a filled 16 px accent round with a check.
 * While the list is in selection mode, every unchecked row shows its ring too.
 *
 * The Status icon is a SIBLING of the checkbox (a checkbox's children are
 * presentational), so a screen reader still announces it; it stays in the DOM
 * (visually hidden) while the ring shows. The checkbox overlays the whole cell,
 * so clicking the glyph selects the row; its tooltip carries the status
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
  const ringMode = selected || selecting;
  const title = status ? `${label} · ${statusTitle(status, statusReason)}` : label;
  const ringColor = status ? `border-current ${statusSpec(status).colorClass}` : "border-fg-3";
  return (
    <span className="relative grid h-4 w-4 shrink-0 place-items-center">
      {status && (
        <StatusIcon
          status={status}
          reason={statusReason}
          testId={dotTestId}
          className={ringMode ? "sr-only" : "group-hover:hidden"}
        />
      )}
      <span
        role="checkbox"
        aria-checked={selected}
        aria-label={label}
        title={title}
        data-testid={testId}
        data-select-mode={ringMode ? "ring" : "glyph"}
        onClick={(e) => {
          e.stopPropagation();
          onSelect(e);
        }}
        className="absolute inset-0 grid cursor-pointer place-items-center"
      >
        {selected ? (
          <span
            className="grid h-4 w-4 place-items-center rounded-full bg-acc text-on-acc"
            data-testid="select-check"
          >
            <Check size={11} strokeWidth={3} />
          </span>
        ) : (
          <span
            aria-hidden="true"
            data-testid="select-ring"
            className={`h-3.5 w-3.5 rounded-full border-2 ${ringColor} ${
              selecting ? "block" : "hidden group-hover:block"
            }`}
          />
        )}
      </span>
    </span>
  );
}
