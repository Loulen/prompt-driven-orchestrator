import { useState } from "react";
import SelectControl from "../../../../shared/ui/SelectControl";
import { CombinedIcon } from "../../StatsAbsorption";
import type { MasterSelection } from "../../../store/absorptionSelection";
import type { AbsorbableRow } from "../../../model/statsAbsorption";

export function MasterList<T extends AbsorbableRow>({
  rows,
  selected,
  valueLabel,
  onSelect,
  ariaLabel = "Spenders",
  monoName = false,
  selection,
  badge,
}: {
  rows: T[];
  selected: string | null;
  /** A node, not a string: a partially filtered Performance row renders an
   *  italic « filtered » here instead of a number (#810). */
  valueLabel: (row: T) => React.ReactNode;
  onSelect: (id: string | null) => void;
  ariaLabel?: string;
  /** Model ids are ids — render them mono (ADR-0065 §2). */
  monoName?: boolean;
  /** The multi-select of the absorption (#890): Pipelines, or Models on the
   *  « By model » axis (#892). A plain click still opens the detail; the ring,
   *  Ctrl/Cmd-click, Shift-click and Space select. Absent where the rows do not
   *  combine (« By project »). */
  selection?: MasterSelection;
  /** A discreet mark after a row's name — the « ⑂ N » of a root Pipeline
   *  under « roll up child runs ». */
  badge?: (row: T) => React.ReactNode;
}) {
  const options = [{ id: "__total__", name: "Total" } as T, ...rows];
  const order = rows.map((row) => row.id);
  const selectedIndex = Math.max(
    0,
    options.findIndex((row) =>
      selected === null ? row.id === "__total__" : row.id === selected,
    ),
  );
  const [focusIndex, setFocusIndex] = useState(selectedIndex);
  const activeFocusIndex = Math.min(focusIndex, options.length - 1);

  return (
    <div
      role="listbox"
      aria-label={ariaLabel}
      className="sx-rank"
      onKeyDown={(event) => {
        if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          event.preventDefault();
          const delta = event.key === "ArrowDown" ? 1 : -1;
          setFocusIndex(
            (activeFocusIndex + delta + options.length) % options.length,
          );
        } else if (event.key === "Enter") {
          event.preventDefault();
          const row = options[activeFocusIndex];
          onSelect(row.id === "__total__" ? null : row.id);
        } else if (event.key === "Backspace" || event.key === "ArrowLeft") {
          event.preventDefault();
          onSelect(null);
        } else if (event.key === " " && selection) {
          event.preventDefault();
          const row = options[activeFocusIndex];
          if (selection.isSelectable(row.id)) selection.toggle(row.id, false);
        }
      }}
    >
      {options.map((row, index) => {
        const isSelected = row.id === (selected ?? "__total__");
        const isTotal = row.id === "__total__";
        const selectable = selection?.isSelectable(row.id) ?? false;
        const checked =
          selectable && (selection?.selected.has(row.id) ?? false);
        const absorbed = row.absorbed?.length ?? 0;
        return (
          <button
            key={row.id}
            type="button"
            role="option"
            aria-selected={isSelected}
            data-checked={selectable ? checked : undefined}
            tabIndex={index === activeFocusIndex ? 0 : -1}
            onFocus={() => setFocusIndex(index)}
            onClick={(event) => {
              if (
                selection &&
                selectable &&
                (event.ctrlKey || event.metaKey || event.shiftKey)
              ) {
                selection.toggle(row.id, event.shiftKey, order);
                return;
              }
              onSelect(isTotal ? null : row.id);
            }}
            data-flash={selection?.flashId === row.id ? "true" : undefined}
            title={row.name}
            className="group sx-rank__row"
          >
            {selection &&
              (selectable ? (
                <SelectControl
                  selected={checked}
                  dotClass={null}
                  label={`Select ${row.name}`}
                  testId="stats-row-select"
                  onSelect={(event) =>
                    selection.toggle(row.id, event.shiftKey, order)
                  }
                />
              ) : (
                <span className="w-4 shrink-0" aria-hidden="true" />
              ))}
            <span className={`sx-rank__name ${monoName ? "font-mono" : ""}`}>
              {row.name}
            </span>
            {!isTotal && badge?.(row)}
            {selection && absorbed > 0 && (
              <CombinedIcon
                count={absorbed}
                dimension={selection.dimension}
                onOpen={() => selection.onOpenMembers(row.id)}
              />
            )}
            <span className="sx-rank__v">
              {isTotal ? "" : valueLabel(row)}
            </span>
          </button>
        );
      })}
    </div>
  );
}
