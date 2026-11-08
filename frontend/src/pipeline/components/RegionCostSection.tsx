/**
 * The money of a loop region in its inspector (ADR draft
 * plafond-de-cout-de-boucle): the « Loop cost » section on a Run, the
 * `max_cost_usd` field of a bounded loop, and the note that a member with no
 * cost source may let the loop overshoot its cap.
 */
import { useState } from "react";
import type React from "react";
import { Info, TriangleAlert, X } from "lucide-react";
import { Field, IconButton, Section, TextInput } from "../../shared/ui/kit";
import type { LoopCost } from "../../shared/generated/LoopCost";
import type { LoopRegion } from "../../shared/generated/LoopRegion";
import { formatCostAmount } from "../../run/model/costLabel";
import { costCapError, costNearCap, formatCostCap, loopCostWindow } from "../../run/model/loopCostLabel";

/** « Loop cost » on a Run: spent / cap, the share of the cap, a 3 px bar. */
export function LoopCostSection({
  cost,
  capUsd,
  currentIter,
}: {
  cost: LoopCost | undefined;
  capUsd: number | null;
  currentIter: number;
}) {
  const spent = cost?.usd ?? null;
  const amount = cost ? formatCostAmount(cost.usd, cost.partial, cost.estimated) : "—";
  const window = loopCostWindow(false, Math.max(currentIter, 1));
  const ratio = spent != null && capUsd != null && capUsd > 0 ? spent / capUsd : null;
  const warn = costNearCap(spent, capUsd);
  return (
    <Section title="Loop cost">
      <div className="ei-loopcost" data-testid="region-loop-cost">
        <div className="ei-loopcost__line">
          <span className="d-mono ei-loopcost__amount" data-warn={warn || undefined}>
            {amount}
            {capUsd != null && ` / ${formatCostCap(capUsd)}`}
          </span>
          <span className="ei-loopcost__meta">
            {ratio != null ? `${Math.round(ratio * 100)} % of cap · ${window}` : window}
          </span>
        </div>
        {ratio != null && (
          <div className="ei-loopcost__bar" data-warn={warn || undefined} aria-hidden>
            <span style={{ width: `${Math.min(ratio, 1) * 100}%` }} />
          </div>
        )}
      </div>
    </Section>
  );
}

/** The value the store keeps for what the author typed: a number, a `$var`, or no cap. */
function capValue(raw: string): number | string | null {
  const value = raw.trim();
  if (value === "") return null;
  return value.startsWith("$") ? value : Number(value);
}

/**
 * The `max_cost_usd` row of « Bound ». The text the author types stays local
 * until it is valid: a refused value (« Must be above 0 », « A number or a
 * $var ») never reaches the pipeline.
 */
export function CostCapField({
  region,
  variables,
  onChange,
  inputRef,
  resolved = null,
}: {
  region: LoopRegion;
  /** The pipeline's declared variables, for a `$var`. */
  variables: readonly string[];
  onChange: (value: number | string | null) => void;
  /** The input, for `raise cap` to focus it. */
  inputRef?: React.Ref<HTMLInputElement>;
  /**
   * On a Run, the amount a `$var` cap resolves to: the field shows it (`5`,
   * not `$budget`), while the YAML keeps the expression until it is retyped.
   */
  resolved?: number | null;
}) {
  const declared = region.max_cost_usd == null ? "" : String(region.max_cost_usd);
  const showsResolved = resolved != null && declared.startsWith("$");
  const stored = showsResolved ? String(resolved) : declared;
  // What the author is typing, while it differs from the pipeline: kept while
  // it is refused, dropped on blur once valid so an undo shows through.
  const [draft, setDraft] = useState<string | null>(null);
  const text = draft ?? stored;
  const error = costCapError(text, variables);
  return (
    <Field label="max_cost_usd" help="Stops the loop at the next lap once its cost reaches this amount" error={error}>
      <span className="ei-cap" data-invalid={error ? "" : undefined}>
        <span className="ei-cap__prefix d-mono" aria-hidden>
          $
        </span>
        <TextInput
          ref={inputRef}
          mono
          value={text}
          placeholder="no cap"
          data-testid="region-max-cost"
          title={showsResolved ? `${declared} = ${resolved}` : undefined}
          onChange={(e) => {
            const next = e.target.value;
            setDraft(next);
            if (costCapError(next, variables) == null) onChange(capValue(next));
          }}
          onBlur={() => {
            if (draft != null && costCapError(draft, variables) == null) setDraft(null);
          }}
        />
        {text !== "" && (
          <IconButton
            icon={X}
            size="sm"
            label="Remove the cost cap"
            data-testid="region-max-cost-clear"
            onClick={() => {
              setDraft(null);
              onChange(null);
            }}
          />
        )}
      </span>
    </Field>
  );
}

/** « Cap may be overshot »: a member runs on a harness PDO cannot read the cost of. */
export function CapOvershootNote({ members }: { members: string[] }) {
  if (members.length === 0) return null;
  return (
    <div className="ei-note ei-note--await" data-testid="region-cap-overshoot">
      <TriangleAlert size={12} strokeWidth={1.5} aria-hidden />
      <span>
        <strong>Cap may be overshot.</strong> {members.join(", ")} {members.length === 1 ? "runs" : "run"} on a harness
        with no cost source: {members.length === 1 ? "its" : "their"} spend does not count toward the cap.
      </span>
    </div>
  );
}

/** A grey informative line under a section (a collection, a followed Run). */
export function RegionInfoNote({ text, testid }: { text: React.ReactNode; testid: string }) {
  return (
    <div className="ei-note ei-note--info" data-testid={testid}>
      <Info size={12} strokeWidth={1.5} aria-hidden />
      <span>{text}</span>
    </div>
  );
}

/** An amber line: a cap typed under the spend, or a Run waiting « budget reached ». */
export function RegionAwaitNote({
  icon: Icon = TriangleAlert,
  children,
  testid,
}: {
  icon?: typeof TriangleAlert;
  children: React.ReactNode;
  testid: string;
}) {
  return (
    <div className="ei-note ei-note--await" data-testid={testid}>
      <Icon size={12} strokeWidth={1.5} aria-hidden />
      <span>{children}</span>
    </div>
  );
}
