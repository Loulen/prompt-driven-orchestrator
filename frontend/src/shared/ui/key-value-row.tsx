import type { ReactNode } from "react";

/** A read-only label → value row, as the Settings sections show them. */
export function KeyValueRow({
  label,
  children,
  tone = "neutral",
  testId,
}: {
  label: ReactNode;
  children: ReactNode;
  tone?: "neutral" | "failed";
  testId?: string;
}) {
  return (
    <div
      className={`flex items-center justify-between gap-4 rounded px-2.5 py-1.5 ${
        tone === "failed" ? "bg-st-failed/14 text-st-failed" : "bg-surface-2 text-text-3"
      }`}
      style={{ fontSize: "10.5px" }}
      data-testid={testId}
    >
      <span className="shrink-0">{label}</span>
      <span className={`min-w-0 truncate text-right ${tone === "failed" ? "" : "text-text-2"}`}>
        {children}
      </span>
    </div>
  );
}
