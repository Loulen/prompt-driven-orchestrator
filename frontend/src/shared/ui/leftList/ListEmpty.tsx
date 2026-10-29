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
  testId,
  children,
}: {
  icon: Icon;
  title: string;
  actionLabel?: string;
  onAction?: () => void;
  actionIcon?: Icon;
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
            <Button size="sm" icon={actionIcon} onClick={onAction}>
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
