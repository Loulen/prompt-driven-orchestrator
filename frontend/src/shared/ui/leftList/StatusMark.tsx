import type { KeyboardEvent, MouseEvent, ReactNode } from "react";
import { Check } from "lucide-react";

interface Props {
  /** The row's status as an icon (`StatusIcon`), or any 14 px glyph. */
  glyph: ReactNode;
  /** Ticked for the bulk bar. */
  selected: boolean;
  /** Some row of the list is ticked: every mark shows its box. */
  selecting?: boolean;
  /** Accessible name of the control: « Select <row> ». */
  label: string;
  /** Its tooltip: the status and its reason, then what a click does. */
  title?: string;
  /** Shift-click extends a range: the event goes to the caller. */
  onSelect: (e: MouseEvent | KeyboardEvent) => void;
  testId?: string;
}

/**
 * The leading mark of a row of the left panel: the status at rest, a box on
 * hover, while some row is ticked, and once ticked (#577). One target: the
 * click selects, it never opens the row.
 */
export default function StatusMark({ glyph, selected, selecting, label, title, onSelect, testId }: Props) {
  return (
    <span
      role="checkbox"
      aria-checked={selected}
      aria-label={label}
      title={title ?? label}
      tabIndex={0}
      className="lp-mark"
      data-selecting={selecting || undefined}
      data-testid={testId}
      onClick={(e) => {
        e.stopPropagation();
        onSelect(e);
      }}
      onKeyDown={(e) => {
        if (e.key !== " " && e.key !== "Enter") return;
        e.preventDefault();
        e.stopPropagation();
        onSelect(e);
      }}
    >
      <span className="lp-mark__glyph">{glyph}</span>
      <span className="lp-mark__box" aria-hidden="true">
        {selected && <Check size={10} strokeWidth={2.5} data-testid="select-check" />}
      </span>
    </span>
  );
}
