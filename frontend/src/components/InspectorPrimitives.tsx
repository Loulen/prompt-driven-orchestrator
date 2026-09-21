import type { BranchFetchError, SourceDrift } from "../types";
import { driftAxes, driftChip, driftTooltip } from "../lib/sourceDrift";

export function SectionHead({
  title,
  count,
  onAdd,
  addTestId,
}: {
  title: string;
  count?: number;
  onAdd?: () => void;
  /** #823: gives the `+ Add` a stable target a tour can aim the Projecteur at. */
  addTestId?: string;
}) {
  return (
    <div className="flex items-center justify-between border-b border-line-soft pb-1 pt-1">
      <span className="font-medium text-fg-3" style={{ fontSize: "10px", textTransform: "uppercase", letterSpacing: "0.05em" }}>
        {title}
        {count !== undefined && (
          <span className="ml-1.5 rounded bg-bg-4 px-1 text-fg-4">{count}</span>
        )}
      </span>
      {onAdd && (
        <button
          onClick={onAdd}
          data-testid={addTestId}
          className="cursor-pointer text-fg-4 hover:text-acc"
          style={{ fontSize: "10px" }}
        >
          + Add
        </button>
      )}
    </div>
  );
}

export function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <label className="mb-0.5 block text-fg-3" style={{ fontSize: "10px" }}>
        {label}
      </label>
      {children}
    </div>
  );
}

/**
 * The **dérive de la source** of a Run, as `n↑ m↓` (#803, ADR-0070 §4).
 *
 * Bicolour on purpose, and the colours are not decoration: `n↑` is blue — the same
 * "running" hue the form's *ahead* chip uses — because it is what the Run produced;
 * `m↓` is amber — the form's *behind* hue — because it is what waits for it at the
 * source. Someone who has read the launch form already knows what each half means.
 *
 * Lives here rather than in either panel because the SAME chip renders in two of
 * them (the Info tab's Source block and the Repositories tab's primary row). Two
 * copies would be free to disagree about what `1↑ 3↓` counts — the #571 lesson.
 *
 * Inert to the click on purpose (design Q5): hovering explains, clicking does
 * nothing. A chip that silently navigated to a diff would make hovering feel risky.
 */
export function SourceDriftChip({
  drift,
  runId,
  fetchError,
  testid = "source-drift-chip",
}: {
  drift: SourceDrift | null;
  runId: string;
  /** A failed fetch in the Run view: the `m` axis becomes `?`, `n` survives. */
  fetchError?: BranchFetchError | null;
  testid?: string;
}) {
  const chip = driftChip(drift, fetchError ?? null);
  if (!chip) return null;
  const tooltip = driftTooltip(drift, runId, fetchError ?? null).join("\n");

  if (chip.kind === "unavailable") {
    return (
      <span
        className="shrink-0 rounded bg-bg-4 px-1 py-0.5 font-medium text-fg-4"
        style={{ fontSize: "9.5px" }}
        // Grey, and no error glyph: a cleaned-up Run branch is the expected end of
        // a Run's life, not a fault to raise an eyebrow at.
        title={tooltip}
        data-testid={testid}
        data-drift="unavailable"
      >
        unavailable
      </span>
    );
  }

  const axes = driftAxes(chip);
  return (
    <span
      className="flex shrink-0 items-center gap-1 rounded bg-bg-4 px-1 py-0.5 font-mono font-medium"
      style={{ fontSize: "9.5px" }}
      title={tooltip}
      data-testid={testid}
      data-drift={chip.kind}
    >
      <span className="text-st-running">{axes?.ahead}</span>
      <span className={chip.kind === "unknown" ? "text-fg-4" : "text-st-await"}>
        {axes?.behind}
      </span>
    </span>
  );
}
