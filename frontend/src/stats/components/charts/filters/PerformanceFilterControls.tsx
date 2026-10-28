import { Check, Info } from "lucide-react";
import { Tooltip } from "../../../../shared/ui/tooltip";
import {
  ALL_NODE_KINDS,
  DURATION_MODES,
  type DurationMode,
  type NodeKind,
} from "../../../model/statsFilters";
import { DECLARED_WAIT_COPY } from "../../../model/performance";

const DURATION_MODE_LABEL: Record<DurationMode, string> = {
  total: "Total",
  active: "Active",
  waiting: "Waiting",
};

/** The **mode de durée** (#819): three readings of the same executions, all
 *  three visible at once and one click apart. Same radiogroup idiom as the zoom
 *  control beside it — ← → walk the modes, roving tabindex — because a select
 *  would hide two of the three states behind a click. */
export function DurationModeSegments({
  value,
  onChange,
}: {
  value: DurationMode;
  onChange: (mode: DurationMode) => void;
}) {
  return (
    <div
      role="radiogroup"
      aria-label="Duration mode"
      data-testid="stats-duration-mode"
      className="d-seg d-seg--sm"
      onKeyDown={(event) => {
        if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
        event.preventDefault();
        const delta = event.key === "ArrowRight" ? 1 : -1;
        const index = DURATION_MODES.indexOf(value);
        const next =
          DURATION_MODES[
            (index + delta + DURATION_MODES.length) % DURATION_MODES.length
          ];
        onChange(next);
        event.currentTarget
          .querySelector<HTMLButtonElement>(`[data-duration-mode="${next}"]`)
          ?.focus();
      }}
    >
      {DURATION_MODES.map((mode) => {
        const selected = mode === value;
        return (
          <button
            key={mode}
            type="button"
            role="radio"
            aria-checked={selected}
            data-duration-mode={mode}
            tabIndex={selected ? 0 : -1}
            onClick={() => onChange(mode)}
            className="d-seg__btn"
          >
            {DURATION_MODE_LABEL[mode]}
          </button>
        );
      })}
    </div>
  );
}

/** The wait coverage (#819), beside the mode in Active and Waiting only: in
 *  Total there is nothing to qualify. A description, not a control — hover or
 *  focus opens the same sentence, which explains why a whole period can read
 *  Active = Total and Waiting = 0. */
export function WaitCoverage({
  waited,
  executions,
}: {
  waited: number;
  executions: number;
}) {
  const copy = `${waited} executions of ${executions} declared at least one wait — for the others, Active = Total and Waiting = 0. ${DECLARED_WAIT_COPY}`;
  return (
    <Tooltip content={copy} side="top">
      <span
        role="img"
        aria-label={copy}
        data-testid="stats-wait-coverage"
        className="inline-flex text-text-3"
      >
        <Info size={12} />
      </span>
    </Tooltip>
  );
}

/** The node kind chips (#810): a multi-selection, every kind checked by default,
 *  with the count of Node rows each covers. */
export function NodeKindChips({
  counts,
  nodeKinds,
  onNodeKindsChange,
}: {
  counts: Record<NodeKind, number>;
  nodeKinds: NodeKind[];
  onNodeKindsChange: (kinds: NodeKind[]) => void;
}) {
  const toggleKind = (kind: NodeKind) =>
    onNodeKindsChange(
      nodeKinds.includes(kind)
        ? nodeKinds.filter((item) => item !== kind)
        : [...ALL_NODE_KINDS].filter(
            (item) => item === kind || nodeKinds.includes(item),
          ),
    );
  return (
    <>
      <span className="sx-filt__label">Node kind</span>
      {ALL_NODE_KINDS.map((kind) => {
        const on = nodeKinds.includes(kind);
        return (
          <button
            key={kind}
            type="button"
            aria-pressed={on}
            data-testid={`stats-node-kind-chip-${kind}`}
            onClick={() => toggleKind(kind)}
            className="sx-toggle capitalize"
          >
            {on && <Check size={11} strokeWidth={2} aria-hidden="true" />}
            {kind}
            <span className="sx-toggle__n">{counts[kind]}</span>
          </button>
        );
      })}
    </>
  );
}
