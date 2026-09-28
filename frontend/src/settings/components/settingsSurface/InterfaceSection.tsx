import { useState } from "react";
import GridSizePicker from "../../../components/GridSizePicker";
import ThemeSelect from "../../../components/ThemeSelect";
import { loadChildRunsExpanded, saveChildRunsExpanded } from "../../../lib/uiPrefs";
import { useEditStore } from "../../../pipeline/store/editStore";
import { useWiringStore } from "../../../stores/wiringStore";
import type { SettingsSection } from "../../model/settingsCategories";
import Section from "./Section";

/**
 * Per-client UI preferences (#342, #759). Both controls persist to localStorage AT
 * THE CHANGE — NOT behind the instance form's Save, and never part of the dirty set.
 * Rendered even when `GET /settings` failed (Trap A).
 */
export default function InterfaceSection({ section }: { section: SettingsSection }) {
  const singleTabMode = useEditStore((s) => s.singleTabMode);
  const setSingleTabMode = useEditStore((s) => s.setSingleTabMode);
  // #783 — « Child runs » default. Read once from localStorage, written at the
  // change; the run list reads the key when it mounts (a reload applies it).
  const [childRunsExpanded, setChildRunsExpanded] = useState(loadChildRunsExpanded);
  const pickChildRuns = (v: boolean) => {
    saveChildRunsExpanded(v);
    setChildRunsExpanded(v);
  };
  // #844 — « Wiring grid ». Held in the wiring store rather than in local state so
  // a change reaches an already-mounted canvas; the store persists it per browser.
  const gridFeedback = useWiringStore((s) => s.gridFeedback);
  const setGridFeedback = useWiringStore((s) => s.setGridFeedback);
  const defaultGridSize = useWiringStore((s) => s.defaultGridSize);
  const setDefaultGridSize = useWiringStore((s) => s.setDefaultGridSize);

  return (
    <Section section={section}>
      <div className="flex flex-col gap-1.5">
        <div className="flex items-center gap-2">
          <span className="font-medium text-fg-2" style={{ fontSize: "11.5px" }}>
            Theme
          </span>
          <span
            className="rounded-full border border-acc-border bg-acc-bg px-2 py-0.5 text-acc"
            style={{ fontSize: "9.5px" }}
            data-testid="setting-theme-badge"
          >
            Device-local · saved immediately
          </span>
        </div>
        <ThemeSelect />
        <div className="text-fg-3" style={{ fontSize: "10.5px" }}>
          With nothing chosen, PDO paints dark — the look it has always had.
          System follows your operating system's light/dark setting and keeps
          following it. Light and dark pin PDO, whatever the system does.
        </div>
      </div>
      <div className="flex flex-col gap-1.5">
        <div className="flex items-center gap-2">
          <span className="font-medium text-fg-2" style={{ fontSize: "11.5px" }}>
            Single-tab mode
          </span>
          <span
            className="rounded-full border border-acc-border bg-acc-bg px-2 py-0.5 text-acc"
            style={{ fontSize: "9.5px" }}
            data-testid="setting-tabs-disabled-badge"
          >
            Device-local · saved immediately
          </span>
        </div>
        <div className="flex items-center gap-2.5">
          <button
            type="button"
            role="switch"
            aria-checked={singleTabMode}
            aria-label="Single-tab mode"
            data-testid="setting-tabs-disabled"
            onClick={() => setSingleTabMode(!singleTabMode)}
            className={`relative h-3.5 w-6 shrink-0 rounded-full transition-colors ${
              singleTabMode ? "bg-acc" : "bg-fg-5"
            }`}
          >
            <span
              className={`absolute top-0.5 h-2.5 w-2.5 rounded-full bg-bg-1 transition-all ${
                singleTabMode ? "left-3" : "left-0.5"
              }`}
            />
          </button>
          <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
            Opening a pipeline or run replaces the current tab instead of stacking a new one.
            Enabling it closes the other open tabs.
          </span>
        </div>
        <div className="text-fg-3" style={{ fontSize: "10.5px" }}>
          Stored in this browser's localStorage. Not shared with other browsers or the daemon.
        </div>
      </div>
      <div className="flex flex-col gap-1.5">
        <div className="flex items-center gap-2">
          <span className="font-medium text-fg-2" style={{ fontSize: "11.5px" }}>
            Child runs
          </span>
          <span
            className="rounded-full border border-acc-border bg-acc-bg px-2 py-0.5 text-acc"
            style={{ fontSize: "9.5px" }}
            data-testid="setting-child-runs-badge"
          >
            Device-local · saved immediately
          </span>
        </div>
        <div
          role="radiogroup"
          aria-label="Child runs"
          className="flex gap-1"
          data-testid="setting-child-runs"
        >
          {(
            [
              { v: true, id: "expanded", label: "Expanded by default" },
              { v: false, id: "collapsed", label: "Collapsed by default" },
            ] as const
          ).map(({ v, id, label }) => {
            const selected = childRunsExpanded === v;
            return (
              <button
                key={id}
                type="button"
                role="radio"
                aria-checked={selected}
                tabIndex={selected ? 0 : -1}
                data-testid={`setting-child-runs-${id}`}
                onClick={() => pickChildRuns(v)}
                onKeyDown={(e) => {
                  if (["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(e.key)) {
                    e.preventDefault();
                    pickChildRuns(!v);
                  }
                }}
                className={`flex flex-1 cursor-pointer items-center justify-center rounded border px-2 py-1.5 font-medium transition-colors ${
                  selected
                    ? "border-acc bg-acc-bg text-acc"
                    : "border-line-strong bg-bg-3 text-fg-4 hover:text-fg-3"
                }`}
                style={{ fontSize: "10px" }}
              >
                {label}
              </button>
            );
          })}
        </div>
        <div className="text-fg-3" style={{ fontSize: "10.5px" }}>
          Applied when the run list loads. Toggling a row or the expand/collapse-all button only
          affects the current session.
        </div>
      </div>
      <div className="flex flex-col gap-1.5">
        <div className="flex items-center gap-2">
          <span className="font-medium text-fg-2" style={{ fontSize: "11.5px" }}>
            Wiring grid
          </span>
          <span
            className="rounded-full border border-acc-border bg-acc-bg px-2 py-0.5 text-acc"
            style={{ fontSize: "9.5px" }}
            data-testid="setting-wiring-grid-badge"
          >
            Device-local · saved immediately
          </span>
        </div>
        <div
          role="radiogroup"
          aria-label="Wiring grid"
          className="flex gap-1"
          data-testid="setting-wiring-grid"
        >
          {(
            [
              { v: "none", label: "Hidden" },
              { v: "dots", label: "Dots" },
              { v: "lines", label: "Lines" },
            ] as const
          ).map(({ v, label }, i, all) => {
            const selected = gridFeedback === v;
            return (
              <button
                key={v}
                type="button"
                role="radio"
                aria-checked={selected}
                tabIndex={selected ? 0 : -1}
                data-testid={`setting-wiring-grid-${v}`}
                onClick={() => setGridFeedback(v)}
                onKeyDown={(e) => {
                  if (["ArrowLeft", "ArrowUp"].includes(e.key)) {
                    e.preventDefault();
                    setGridFeedback(all[(i + all.length - 1) % all.length].v);
                  }
                  if (["ArrowRight", "ArrowDown"].includes(e.key)) {
                    e.preventDefault();
                    setGridFeedback(all[(i + 1) % all.length].v);
                  }
                }}
                className={`flex flex-1 cursor-pointer items-center justify-center rounded border px-2 py-1.5 font-medium transition-colors ${
                  selected
                    ? "border-acc bg-acc-bg text-acc"
                    : "border-line-strong bg-bg-3 text-fg-4 hover:text-fg-3"
                }`}
                style={{ fontSize: "10px" }}
              >
                {label}
              </button>
            );
          })}
        </div>
        <div className="text-fg-3" style={{ fontSize: "10.5px" }}>
          How the wiring grid shows itself while you draw an edge on the canvas.
        </div>
      </div>
      <div className="flex flex-col gap-1.5">
        <div className="flex items-center gap-2">
          <span className="font-medium text-fg-2" style={{ fontSize: "11.5px" }}>
            Wiring grid size
          </span>
          <span
            className="rounded-full border border-acc-border bg-acc-bg px-2 py-0.5 text-acc"
            style={{ fontSize: "9.5px" }}
            data-testid="setting-wiring-grid-size-badge"
          >
            Device-local · saved immediately
          </span>
        </div>
        <GridSizePicker
          value={defaultGridSize}
          onChange={(v) => v && setDefaultGridSize(v)}
          ariaLabel="Wiring grid size"
          testId="setting-wiring-grid-size"
        />
        <div className="text-fg-3" style={{ fontSize: "10.5px" }}>
          The step edges snap to on the canvas, for every pipeline that has no size of its own. A
          pipeline can pick its own size in its Pipeline Inspector; that choice is saved in the
          pipeline file and wins over this one. Existing routes are never re-snapped.
        </div>
      </div>
    </Section>
  );
}
