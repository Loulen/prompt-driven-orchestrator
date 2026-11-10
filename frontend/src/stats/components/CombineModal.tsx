import { useEffect, useRef, useState } from "react";
import { Check, Combine } from "lucide-react";
import {
  activity,
  defaultAbsorbent,
  MONO_DIMENSIONS,
  NOUN,
  plural,
  type AbsorbableRow,
  type AbsorptionCount,
  type AbsorptionDimension,
} from "../model/statsAbsorption";
import { ModalFrame } from "./StatsAbsorption";

/**
 * Pick the absorbent among the selected rows. ↑/↓ change the choice, Enter
 * combines, Escape cancels (the host routes Escape — see `useStatsAbsorption`).
 */
/** What Stats will do with the combined rows, said per dimension. */
function combineSubtitle(
  dimension: AbsorptionDimension,
  scopeName: string | undefined,
): string {
  if (dimension === "node")
    return `Stats will read them as one node${scopeName ? ` of ${scopeName}` : ""}, in every tab.`;
  if (dimension === "model")
    return "Stats will read them as one model in Cost and Performance, the couples of every node included.";
  if (dimension === "effort")
    return `Stats will read them as one effort of ${scopeName ?? "this model"}, in Cost and Performance, the couples of every node included.`;
  if (dimension === "couple")
    return `Stats will read them as one couple of ${scopeName ?? "this node"} only, in Cost and Performance.`;
  return "Stats will read them as one pipeline, in every tab.";
}

export function CombineModal<T extends AbsorbableRow>({
  rows,
  count,
  dimension = "pipeline",
  scopeName,
  preferred,
  busy,
  error,
  onCancel,
  onCombine,
}: {
  rows: T[];
  count: AbsorptionCount<T>;
  dimension?: AbsorptionDimension;
  /** The Pipeline a Node absorption lives under, the model of an effort
   *  absorption, the Node of a couple absorption — by name. */
  scopeName?: string;
  /** Rows the default absorbent prefers (an explicit effort, #906). */
  preferred?: (row: T) => boolean;
  busy: boolean;
  error: string | null;
  onCancel: () => void;
  onCombine: (absorbent: T) => void;
}) {
  const [chosenId, setChosenId] = useState<string | null>(
    () => defaultAbsorbent(rows, count.of, preferred)?.id ?? null,
  );
  const chosen = rows.find((row) => row.id === chosenId) ?? rows[0];
  const others = rows.filter((row) => row.id !== chosen.id);
  const nameCounts = new Map<string, number>();
  for (const row of rows)
    nameCounts.set(row.name, (nameCounts.get(row.name) ?? 0) + 1);

  const latest = useRef({ rows, chosen, busy, onCombine });
  useEffect(() => {
    latest.current = { rows, chosen, busy, onCombine };
  });
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const { rows, chosen, busy, onCombine } = latest.current;
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        event.stopPropagation();
        const index = rows.findIndex((row) => row.id === chosen.id);
        const delta = event.key === "ArrowDown" ? 1 : -1;
        setChosenId(rows[(index + delta + rows.length) % rows.length].id);
      } else if (event.key === "Enter") {
        event.preventDefault();
        event.stopPropagation();
        if (!busy) onCombine(chosen);
      }
    };
    // Capture on window: the modal answers before the master list behind it
    // (whose Enter would open a detail) ever sees the key.
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);

  const otherLabel = others.every((row) => row.name === chosen.name)
    ? others.length === 1
      ? "The other version"
      : "The other versions"
    : others.map((row) => row.name).join(", ");
  const alreadyCombined = chosen.absorbed?.length ?? 0;

  return (
    <ModalFrame
      testid="stats-combine-modal"
      onDismiss={onCancel}
      icon={<Combine size={14} strokeWidth={1.5} className="sx-acc" aria-hidden="true" />}
      title={`Combine ${rows.length} ${NOUN[dimension]}s`}
      footerNote={
        <span data-testid="stats-combine-consequence" title="Nothing is rewritten: undo it any time from the combined icon on its row.">
          {/* No count here (#906): the absorbent's own count is not the combined
              one, and Runs overlap across members, so no sum would be honest. */}
          {otherLabel} will be counted under{" "}
          <span className="font-mono text-text">{chosen.name}</span>
          {alreadyCombined > 0
            ? `, together with the ${plural(alreadyCombined, NOUN[dimension])} already combined into it`
            : ""}
          . Nothing is rewritten: undo it any time from the combined icon on its
          row.
        </span>
      }
      actions={
        <>
          <button
            type="button"
            onClick={onCancel}
            data-testid="stats-combine-cancel"
            className="d-btn"
          >
            Cancel
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={() => onCombine(chosen)}
            data-testid="stats-combine-confirm"
            className="d-btn d-btn--primary"
            title="Combine (Enter)"
          >
            Combine
          </button>
        </>
      }
    >
      <p className="d-meta m-0">{combineSubtitle(dimension, scopeName)}</p>
      <div className="d-caps">Keep the name of</div>
      <div
        role="radiogroup"
        aria-label="Keep the name of"
        className="flex flex-col gap-1"
      >
        {rows.map((row) => {
          const checked = row.id === chosen.id;
          const notes = [activity(count.of(row), count.word, row.last_run)];
          if ((row.absorbed?.length ?? 0) > 0)
            notes.push(`already combines ${row.absorbed!.length}`);
          if ((nameCounts.get(row.name) ?? 0) > 1)
            notes.push("same name, other version");
          return (
            <button
              key={row.id}
              type="button"
              role="radio"
              aria-checked={checked}
              data-testid="stats-combine-option"
              onClick={() => setChosenId(row.id)}
              className="sx-combine__opt"
            >
              <span className="sx-radio" aria-hidden="true">
                {checked && <Check size={10} strokeWidth={2.5} />}
              </span>
              <span className="sx-mlist__two">
                <span
                  className={`sx-mlist__name ${MONO_DIMENSIONS.has(dimension) ? "font-mono" : ""}`}
                  title={row.name}
                >
                  {row.name}
                </span>
                <span className="d-meta">{notes.join(" · ")}</span>
              </span>
              {checked && <span className="d-chip shrink-0">keeps its name</span>}
            </button>
          );
        })}
      </div>
      {error && (
        <div className="sx-err" data-testid="stats-absorption-error">
          {error}
        </div>
      )}
    </ModalFrame>
  );
}
