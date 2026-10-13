import { statusSpec, statusTitle, type StatusKey } from "../lib/statusTable";

/** sm 12 px (lists, panels), xs 10 px (child counters, iterations), md 16 px (node corner). */
export type StatusIconSize = "xs" | "sm" | "md";

const PX: Record<StatusIconSize, number> = { xs: 10, sm: 12, md: 16 };

interface Props {
  status: StatusKey | string;
  /** Appended to the tooltip (« Failed: <reason> »), e.g. a failure or awaiting reason. */
  reason?: string | null;
  size?: StatusIconSize;
  /** A text label equivalent sits right next to the glyph: hide it from
   *  assistive tech so the status is not announced twice. */
  decorative?: boolean;
  testId?: string;
  className?: string;
}

/**
 * The Status icon (PDO-22): one lucide glyph per status, its SHAPE carrying the
 * status and its colour reinforcing it — see `STATUS_TABLE`. It names itself
 * (`role="img"` + `aria-label`) and its `title` adds the reason when there is
 * one. Running turns slowly (2.4 s a turn), fixed under reduced motion.
 */
export default function StatusIcon({ status, reason, size = "sm", decorative = false, testId, className = "" }: Props) {
  const { glyph: Glyph, colorClass, label, spin } = statusSpec(status);
  const px = PX[size];
  return (
    <span
      role={decorative ? undefined : "img"}
      aria-label={decorative ? undefined : label}
      aria-hidden={decorative ? true : undefined}
      title={statusTitle(status, reason)}
      data-testid={testId}
      data-status={status}
      className={`inline-grid shrink-0 place-items-center ${colorClass} ${className}`}
      style={{ width: px, height: px }}
    >
      <Glyph
        size={px}
        strokeWidth={2.25}
        aria-hidden="true"
        className={spin ? "motion-safe:animate-[spin_2.4s_linear_infinite]" : undefined}
      />
    </span>
  );
}
