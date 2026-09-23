import { GRID_SIZES, gridStep, type GridSize } from "../lib/wiringGrid";

/**
 * The S / M / L wiring-grid size picker (#877 / ADR-0076), shared by the global
 * default (Settings › General › Interface) and the pipeline's own choice (the
 * Pipeline Inspector). With `globalSize` set, a leading « Global » option stands
 * for « no choice of its own » (`null`): the pipeline follows the reader's
 * default, and the file carries no `grid_size`.
 *
 * A radiogroup of buttons, arrow-key navigable — the same control as the other
 * Interface rows.
 */
export default function GridSizePicker({
  value,
  onChange,
  globalSize,
  ariaLabel,
  testId,
}: {
  value: GridSize | null;
  onChange: (v: GridSize | null) => void;
  /** When set, offers « Global » (value `null`), labelled with the size it resolves to. */
  globalSize?: GridSize;
  ariaLabel: string;
  testId: string;
}) {
  const options: { v: GridSize | null; label: string; title: string; id: string }[] = [
    ...(globalSize
      ? [
          {
            v: null,
            label: `Global (${globalSize})`,
            title: `Follow the global default — ${globalSize}, ${gridStep(globalSize)}px`,
            id: "global",
          },
        ]
      : []),
    ...GRID_SIZES.map((s) => ({ v: s, label: s, title: `${s} — ${gridStep(s)}px`, id: s })),
  ];
  return (
    <div role="radiogroup" aria-label={ariaLabel} className="flex gap-1" data-testid={testId}>
      {options.map(({ v, label, title, id }, i, all) => {
        const selected = value === v;
        return (
          <button
            key={id}
            type="button"
            role="radio"
            aria-checked={selected}
            tabIndex={selected ? 0 : -1}
            title={title}
            data-testid={`${testId}-${id}`}
            onClick={() => onChange(v)}
            onKeyDown={(e) => {
              if (["ArrowLeft", "ArrowUp"].includes(e.key)) {
                e.preventDefault();
                onChange(all[(i + all.length - 1) % all.length].v);
              }
              if (["ArrowRight", "ArrowDown"].includes(e.key)) {
                e.preventDefault();
                onChange(all[(i + 1) % all.length].v);
              }
            }}
            className={`flex flex-1 cursor-pointer flex-col items-center justify-center rounded border px-2 py-1.5 font-medium transition-colors ${
              selected
                ? "border-acc bg-acc-bg text-acc"
                : "border-line-strong bg-bg-3 text-fg-4 hover:text-fg-3"
            }`}
            style={{ fontSize: "10px" }}
          >
            <span>{label}</span>
            {v && (
              <span className="font-normal opacity-70" style={{ fontSize: "9px" }}>
                {gridStep(v)}px
              </span>
            )}
          </button>
        );
      })}
    </div>
  );
}
