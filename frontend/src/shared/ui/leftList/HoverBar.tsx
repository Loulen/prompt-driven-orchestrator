import type { ReactNode } from "react";
import { IconButton, type Icon } from "../kit";

/**
 * The hover actions of a row of the left panel: a floating bar over the end
 * of the row, shown on hover and on keyboard focus, so the row's text never
 * moves. Its last entry is the row's « ⋯ » menu.
 */
export default function HoverBar({ children, testId }: { children: ReactNode; testId?: string }) {
  return (
    <span className="lp-hover" data-testid={testId}>
      {children}
    </span>
  );
}

/** One icon of a `HoverBar`: its name in a tooltip; the click stays off the row. */
export function RowAction({
  icon,
  label,
  onClick,
  danger,
  testId,
  disabledReason,
}: {
  icon: Icon;
  label: string;
  onClick: () => void;
  danger?: boolean;
  testId?: string;
  /** Why the caller may not make it (CORE-205): greyed, its tooltip says why. */
  disabledReason?: string;
}) {
  return (
    <IconButton
      icon={icon}
      label={label}
      size="sm"
      tone={danger ? "danger" : undefined}
      data-testid={testId}
      disabledReason={disabledReason}
      onClick={(e) => {
        e.stopPropagation();
        onClick();
      }}
      onKeyDown={(e) => e.stopPropagation()}
    />
  );
}
