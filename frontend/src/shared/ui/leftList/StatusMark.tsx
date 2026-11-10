import type { CSSProperties, KeyboardEvent, MouseEvent, ReactNode } from "react";
import { Check } from "lucide-react";
import { toneColor, type Tone } from "../kit/tones";

interface Props {
  /** The row's status as an icon (`StatusIcon`), or any 14 px glyph. */
  glyph: ReactNode;
  /** The tone of that status: the colour of the ring on hover. None: a neutral ring. */
  tone?: Tone;
  /** Ticked for the bulk bar. */
  selected: boolean;
  /** Some row of the list is ticked: every mark shows its ring. */
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
 * The leading mark of a row of the left panel, the Contrôle de sélection d'une
 * liste (#577): the status at rest; a ring in the status's colour while the
 * row is hovered and while some row is ticked; a full round of the accent with
 * a check once ticked. Round, as `SelectControl` draws it. One target: the
 * click selects, it never opens the row.
 */
export default function StatusMark({ glyph, tone = "neutral", selected, selecting, label, title, onSelect, testId }: Props) {
  return (
    <span
      role="checkbox"
      aria-checked={selected}
      aria-label={label}
      title={title ?? label}
      tabIndex={0}
      className="lp-mark"
      data-selecting={selecting || undefined}
      data-tone={tone}
      style={{ "--mark-tone": toneColor(tone) } as CSSProperties}
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
        {selected && <Check size={11} strokeWidth={3} data-testid="select-check" />}
      </span>
    </span>
  );
}
