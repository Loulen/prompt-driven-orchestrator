import type { ReactNode } from "react";

/**
 * The frame every card that brackets a tour is drawn in (#823, #824): centred
 * on the Projecteur layer, over a fully dimmed window.
 */
export function CardShell({ testId, children }: { testId: string; children: ReactNode }) {
  return (
    <div className="pointer-events-auto absolute inset-0 flex items-center justify-center">
      <div
        data-testid={testId}
        className="w-[420px] rounded-lg border border-line-strong bg-bg-2 p-4 shadow-lg"
        style={{ fontSize: "12px" }}
      >
        {children}
      </div>
    </div>
  );
}
