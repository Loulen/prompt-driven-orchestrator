import type { ReactNode } from "react";

/**
 * The frame every card that brackets a tour is drawn in (#823, #824): centred
 * on the Projecteur layer, over a fully dimmed window. `footer` is its 44 px
 * bottom bar: a meta line on the left, the buttons on the right.
 */
export function CardShell({
  testId,
  label,
  children,
  footer,
}: {
  testId: string;
  label: string;
  children: ReactNode;
  footer: ReactNode;
}) {
  return (
    <div className="pointer-events-auto absolute inset-0 flex items-center justify-center">
      <div data-testid={testId} role="dialog" aria-label={label} className="tr-frame tr-bcard">
        {children}
        <div className="tr-foot">{footer}</div>
      </div>
    </div>
  );
}
