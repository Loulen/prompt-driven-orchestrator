import { useState } from "react";
import { Segmented, Switch } from "../../../shared/ui/kit";
import { loadChildRunsExpanded, saveChildRunsExpanded } from "../../../lib/uiPrefs";
import { GRID_SIZES, gridStep, type GridSize } from "../../../lib/wiringGrid";
import { useEditStore } from "../../../pipeline/store/editStore";
import { useWiringStore } from "../../../stores/wiringStore";
import type { SettingsSection } from "../../model/settingsCategories";
import { SettingRow } from "./fields";
import Section from "./Section";

/**
 * Settings › Appearance › Interface: this browser's behaviour (#342, #783, #844). Every
 * control persists to localStorage AT THE CHANGE — NOT behind the instance form's Save,
 * and never part of the dirty set. The Theme, the Dark mode, the UI scale and Compact
 * nodes are in the sections above. Rendered even when `GET /settings` failed (Trap A).
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
      <SettingRow
        label="Single-tab mode"
        help="Opening a pipeline or a run replaces the current tab; turning it on closes the other tabs"
        scope="device"
        testId="setting-row-tabs-disabled"
      >
        <Switch label="Single-tab mode" checked={singleTabMode} onChange={setSingleTabMode} />
      </SettingRow>
      <SettingRow
        label="Child runs"
        help="How the run list opens; a row's chevron only changes the current session"
        scope="device"
        testId="setting-row-child-runs"
      >
        <Segmented
          label="Child runs"
          size="sm"
          value={childRunsExpanded ? "expanded" : "collapsed"}
          onChange={(v) => pickChildRuns(v === "expanded")}
          items={[
            { value: "expanded", label: "Expanded" },
            { value: "collapsed", label: "Collapsed" },
          ]}
        />
      </SettingRow>
      <SettingRow
        label="Wiring grid"
        help="How the grid shows while you draw an edge on the canvas"
        scope="device"
        testId="setting-row-wiring-grid"
      >
        <Segmented
          label="Wiring grid"
          size="sm"
          value={gridFeedback}
          onChange={setGridFeedback}
          items={[
            { value: "none", label: "Hidden" },
            { value: "dots", label: "Dots" },
            { value: "lines", label: "Lines" },
          ]}
        />
      </SettingRow>
      <SettingRow
        label="Wiring grid size"
        help="The step edges snap to, for every pipeline without a size of its own; existing routes are never re-snapped"
        scope="device"
        testId="setting-row-wiring-grid-size"
      >
        <Segmented<GridSize>
          label="Wiring grid size"
          size="sm"
          value={defaultGridSize}
          onChange={setDefaultGridSize}
          items={GRID_SIZES.map((size) => ({ value: size, label: size, title: `${size}, ${gridStep(size)} px` }))}
        />
      </SettingRow>
    </Section>
  );
}
