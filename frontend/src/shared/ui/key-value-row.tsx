import type { ReactNode } from "react";

/**
 * A read-only label → value row, as the Settings sections show them: the label in a
 * 180 px column, the value after it, 24 px, ruled (`settings.css`).
 */
export function KeyValueRow({
  label,
  children,
  tone = "neutral",
  title,
  testId,
}: {
  label: ReactNode;
  children: ReactNode;
  tone?: "neutral" | "failed";
  /** The tooltip of the value, when it may be cut. */
  title?: string;
  testId?: string;
}) {
  return (
    <div className="set-kv" data-tone={tone === "failed" ? "failed" : undefined} data-testid={testId}>
      <span className="set-kv__k">{label}</span>
      <span className="set-kv__v" title={title}>
        {children}
      </span>
    </div>
  );
}
