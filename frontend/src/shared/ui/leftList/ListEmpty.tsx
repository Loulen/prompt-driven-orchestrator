import type { ReactNode } from "react";
import { Button, EmptyState, type Icon } from "../kit";

/**
 * An empty list of the left panel: an icon, two to four words, one button. No
 * sentence of explanation.
 */
export default function ListEmpty({
  icon,
  title,
  actionLabel,
  onAction,
  actionIcon,
  actionRefused,
  testId,
  children,
}: {
  icon: Icon;
  title: string;
  actionLabel?: string;
  onAction?: () => void;
  actionIcon?: Icon;
  /** CORE-205: why the caller may not make the action (greyed, its tooltip says why), else absent. */
  actionRefused?: string;
  testId?: string;
  /** What the extensions add under it (`runs/empty`). */
  children?: ReactNode;
}) {
  return (
    <div className="lp-empty" data-testid={testId}>
      <EmptyState
        icon={icon}
        action={
          actionLabel && onAction ? (
            <Button size="sm" icon={actionIcon} onClick={onAction} disabledReason={actionRefused}>
              {actionLabel}
            </Button>
          ) : undefined
        }
      >
        {title}
      </EmptyState>
      {children}
    </div>
  );
}
