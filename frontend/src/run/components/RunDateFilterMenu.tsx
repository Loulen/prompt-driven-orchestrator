import { useState } from "react";
import { Popover } from "@base-ui/react/popover";
import { ArrowRight, Calendar, Check, ChevronDown } from "lucide-react";
import {
  DATE_PRESETS,
  dateFilterLabel,
  dateModeHint,
  dayString,
  rangeDays,
  type DateMode,
  type DateRange,
  type RunDateFilter,
} from "../model/runDateFilter";

const MODES: { mode: DateMode; label: string }[] = [
  { mode: "active", label: "Active" },
  { mode: "started", label: "Started" },
];

function sameRange(a: DateRange | null, b: DateRange | null): boolean {
  if (a === null || b === null) return a === b;
  if ("preset" in a) return "preset" in b && a.preset === b.preset;
  return "from" in b && a.from === b.from && a.to === b.to;
}

/**
 * The Date axis of the Runs list's filters: a calendar button that opens the
 * Active / Started switch, the presets and a custom range. Applies to the
 * archived Runs too (the filter runs before the active / archived split).
 */
export default function RunDateFilterMenu({
  value,
  onChange,
}: {
  value: RunDateFilter;
  onChange: (v: RunDateFilter) => void;
}) {
  const [open, setOpen] = useState(false);
  const now = new Date();
  const label = dateFilterLabel(value, now);
  const custom = value.range ?? { from: dayString(now), to: dayString(now) };
  const days = rangeDays(custom, now);
  const setCustom = (edge: "from" | "to", day: string) => {
    if (day.length === 0) return;
    onChange({ ...value, range: { ...days, [edge]: day } });
  };
  const option = (key: string, text: string, range: DateRange | null) => {
    const selected = sameRange(value.range, range);
    return (
      <button
        key={key}
        type="button"
        data-testid={`run-filter-date-${key}`}
        aria-pressed={selected}
        className={`flex w-full cursor-pointer items-center justify-between rounded px-2 py-1.5 text-left text-fg-2 transition-colors hover:bg-bg-4 ${
          selected ? "bg-bg-4 text-fg" : ""
        }`}
        style={{ fontSize: "11px" }}
        onClick={() => {
          onChange({ ...value, range });
          setOpen(false);
        }}
      >
        {text}
        {selected && <Check size={11} className="text-acc" />}
      </button>
    );
  };

  return (
    <Popover.Root open={open} onOpenChange={setOpen}>
      <Popover.Trigger
        data-testid="run-filter-date"
        title={label ? dateModeHint(value.mode) : "Date"}
        className={`flex min-w-0 max-w-full flex-[1_0_auto] cursor-pointer items-center justify-between gap-1 rounded border px-1.5 py-0.5 text-left outline-none transition-colors hover:bg-bg-4 data-[popup-open]:border-acc ${
          label !== null ? "border-acc bg-bg-3 text-acc" : "border-line-strong bg-bg-3 text-fg-4"
        }`}
        style={{ fontSize: "10px" }}
      >
        <span className="flex min-w-0 items-center gap-1">
          <Calendar size={9} className="shrink-0" />
          <span className="truncate">{label ?? "Date"}</span>
        </span>
        <ChevronDown size={9} className="shrink-0" />
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Positioner className="isolate z-50 outline-none" side="bottom" align="end" sideOffset={4}>
          <Popover.Popup
            data-testid="run-filter-date-menu"
            className="w-[240px] rounded-md border border-line-strong bg-bg-3 p-1 shadow-lg outline-none"
          >
            <div
              role="radiogroup"
              aria-label="Date mode"
              className="mb-1 grid grid-cols-2 gap-0.5 rounded border border-line bg-bg-2 p-0.5"
            >
              {MODES.map(({ mode, label: text }) => (
                <button
                  key={mode}
                  type="button"
                  role="radio"
                  aria-checked={value.mode === mode}
                  data-testid={`run-filter-date-mode-${mode}`}
                  className={`cursor-pointer rounded py-1 text-center transition-colors ${
                    value.mode === mode ? "bg-bg-4 text-fg" : "text-fg-3 hover:text-fg-2"
                  }`}
                  style={{ fontSize: "11px" }}
                  onClick={() => onChange({ ...value, mode })}
                >
                  {text}
                </button>
              ))}
            </div>
            {option("any", "Any time", null)}
            {DATE_PRESETS.map(({ preset, label: text }) => option(preset, text, { preset }))}
            <div className="mt-1 border-t border-line px-2 pb-1 pt-2">
              <div className="mb-1.5 text-fg-4" style={{ fontSize: "10px" }}>
                Custom range
              </div>
              <div className="flex items-center gap-1">
                <input
                  type="date"
                  aria-label="From"
                  data-testid="run-filter-date-from"
                  value={days.from}
                  onChange={(e) => setCustom("from", e.target.value)}
                  className="min-w-0 flex-1 rounded border border-line-strong bg-bg-2 px-1.5 py-1 font-mono text-fg-2 outline-none focus:border-acc"
                  style={{ fontSize: "10px" }}
                />
                <ArrowRight size={10} className="shrink-0 text-fg-3" />
                <input
                  type="date"
                  aria-label="To"
                  data-testid="run-filter-date-to"
                  value={days.to}
                  onChange={(e) => setCustom("to", e.target.value)}
                  className="min-w-0 flex-1 rounded border border-line-strong bg-bg-2 px-1.5 py-1 font-mono text-fg-2 outline-none focus:border-acc"
                  style={{ fontSize: "10px" }}
                />
              </div>
              <p className="mt-2 text-fg-4" style={{ fontSize: "10px" }} data-testid="run-filter-date-hint">
                {dateModeHint(value.mode)}
              </p>
            </div>
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  );
}
