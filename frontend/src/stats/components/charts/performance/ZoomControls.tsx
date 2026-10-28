import type { StatsZoom } from "../../../model/statsFilters";
import { ZOOM_LABEL, ZOOM_LEVELS } from "../../../model/performance";

/** A 16×8 miniature of what the level draws — whiskers to the extremes (Full),
 *  shorter whiskers with a point left outside (Fenced), the bare box (Box). */
export function ZoomGlyph({ level }: { level: StatsZoom }) {
  return (
    <svg
      width="16"
      height="8"
      viewBox="0 0 16 8"
      aria-hidden="true"
      className="shrink-0 overflow-visible"
      data-testid={`stats-zoom-glyph-${level}`}
    >
      {level !== "box" && (
        <line
          x1={level === "full" ? 0.5 : 3}
          x2={level === "full" ? 15.5 : 12}
          y1="4"
          y2="4"
          stroke="currentColor"
          strokeWidth="1"
          opacity="0.55"
        />
      )}
      <rect
        x={level === "box" ? 3.5 : 5.5}
        y="1.5"
        width={level === "box" ? 9 : 5}
        height="5"
        fill="none"
        stroke="currentColor"
        strokeWidth="1"
      />
      <line
        x1="8"
        y1="0.5"
        x2="8"
        y2="7.5"
        stroke="currentColor"
        strokeWidth="1"
      />
      {level === "fenced" && (
        <circle cx="15" cy="4" r="1" fill="currentColor" opacity="0.55" />
      )}
    </svg>
  );
}

/** The three-position zoom control: one radiogroup, roving tabindex, ← → walk
 *  the levels (a select would hide two of the three states behind a click). */
export function ZoomSegments({
  value,
  onChange,
}: {
  value: StatsZoom;
  onChange: (zoom: StatsZoom) => void;
}) {
  return (
    <div
      role="radiogroup"
      aria-label="Box-plot zoom"
      data-testid="stats-performance-zoom"
      className="flex items-center gap-0.5 rounded border border-line bg-surface-2 p-0.5"
      onKeyDown={(event) => {
        if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
        event.preventDefault();
        const delta = event.key === "ArrowRight" ? 1 : -1;
        const index = ZOOM_LEVELS.indexOf(value);
        const next =
          ZOOM_LEVELS[
            (index + delta + ZOOM_LEVELS.length) % ZOOM_LEVELS.length
          ];
        onChange(next);
        // Roving tabindex: the selection carries the focus, so a second arrow
        // press keeps walking instead of falling back to the group.
        event.currentTarget
          .querySelector<HTMLButtonElement>(`[data-zoom="${next}"]`)
          ?.focus();
      }}
    >
      {ZOOM_LEVELS.map((level) => {
        const selected = level === value;
        return (
          <button
            key={level}
            type="button"
            role="radio"
            aria-checked={selected}
            data-zoom={level}
            tabIndex={selected ? 0 : -1}
            onClick={() => onChange(level)}
            className={`flex items-center gap-1.5 rounded px-2 py-0.5 transition-colors ${
              selected ? "bg-surface-2 text-text" : "text-text-3 hover:text-text-2"
            }`}
          >
            <ZoomGlyph level={level} />
            {ZOOM_LABEL[level]}
          </button>
        );
      })}
    </div>
  );
}

/** « Independent scales »: each row on its own axis instead of the shared one.
 *  Same switch idiom as Settings › Interface — this is a per-browser reading
 *  preference, not a daemon knob. */
export function IndependentScalesSwitch({
  checked,
  onChange,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <span className="flex items-center gap-2">
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        aria-label="Independent scales"
        data-testid="stats-performance-independent-scales"
        onClick={() => onChange(!checked)}
        className={`relative h-3.5 w-6 shrink-0 rounded-full transition-colors ${
          checked ? "bg-primary" : "bg-text-3"
        }`}
      >
        <span
          className={`absolute top-0.5 h-2.5 w-2.5 rounded-full bg-surface-0 transition-all ${
            checked ? "left-3" : "left-0.5"
          }`}
        />
      </button>
      <span className="text-text-3">Independent scales</span>
    </span>
  );
}
