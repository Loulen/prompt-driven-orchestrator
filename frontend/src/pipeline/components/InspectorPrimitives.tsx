import type { ReactNode } from "react";
import { Lock } from "lucide-react";
import type { BranchFetchError } from "../../shared/generated/BranchFetchError";
import type { SourceDrift } from "../../shared/generated/SourceDrift";
import { driftAxes, driftChip, driftTooltip } from "../../run/model/sourceDrift";

/**
 * A row of an edit form (`inspector.formRow`): the label in the 96 px column,
 * the control after it. The help goes in the label's tooltip, never under the
 * field. For a lone input, the kit's `Field` ties the label to it.
 */
export function FormRow({
  label,
  help,
  aside,
  children,
  testid,
}: {
  label: string;
  help?: string;
  /** A mark after the label (a frozen lock). */
  aside?: ReactNode;
  children: ReactNode;
  testid?: string;
}) {
  return (
    <div className="d-field" data-testid={testid}>
      <span className="d-field__label ei-line" title={help ?? label} data-help={help ? "" : undefined}>
        <span className="ei-trunc">{label}</span>
        {aside}
      </span>
      <div className="d-field__ctl">{children}</div>
    </div>
  );
}

/** The kit's square switch, with the test id and the tooltip of a form row. */
export function Toggle({
  checked,
  onChange,
  label,
  disabled,
  testid,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
  label: string;
  disabled?: boolean;
  testid?: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      title={label}
      disabled={disabled}
      className="d-switch"
      onClick={() => onChange(!checked)}
      data-testid={testid}
    >
      <span className="d-switch__knob" aria-hidden />
    </button>
  );
}

/** « Frozen at spawn »: a lock after a label, the rest in its tooltip. */
export function FrozenMark({ title, testid }: { title: string; testid?: string }) {
  return (
    <span className="ei-lock" role="img" aria-label={title} title={title} data-testid={testid}>
      <Lock size={10} strokeWidth={1.75} aria-hidden />
    </span>
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
        className="d-chip"
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
      className="d-chip d-chip--mono"
      title={tooltip}
      data-testid={testid}
      data-drift={chip.kind}
    >
      <span className="text-st-running">{axes?.ahead}</span>
      <span className={chip.kind === "unknown" ? "text-text-3" : "text-st-await"}>
        {axes?.behind}
      </span>
    </span>
  );
}
