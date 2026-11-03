import ThemeSelect from "../../../shared/ui/ThemeSelect";
import { useTheme } from "../../../hooks/useTheme";
import { Segmented, Switch } from "../../../shared/ui/kit";
import { setUiScale, UI_SCALES, useUiScale, type UiScale } from "../../../lib/uiScale";
import type { SettingsSection } from "../../model/settingsCategories";
import { SettingRow } from "./fields";
import Section from "./Section";

const SCALE_ITEMS = UI_SCALES.map((scale) => ({ value: String(scale), label: `${scale} %` }));

/**
 * Settings › Appearance › Theme: what changes the whole interface. The Theme and the Dark
 * mode belong to the Actor's Apparence (`Your account`), the UI scale to this browser
 * (`This device`). Each written at the change, out of the instance form; rendered even
 * when `GET /settings` failed.
 */
export default function ThemeSection({ section }: { section: SettingsSection }) {
  const uiScale = useUiScale();
  const { mode, setMode } = useTheme();
  return (
    <Section section={section}>
      <SettingRow label="Theme" keywords="colours colors" scope="account" testId="setting-row-theme">
        <ThemeSelect />
      </SettingRow>
      <SettingRow label="Dark mode" keywords="light" scope="account" testId="setting-row-dark-mode">
        <Switch label="Dark mode" checked={mode === "dark"} onChange={(on) => setMode(on ? "dark" : "light")} />
      </SettingRow>
      <SettingRow
        label="UI scale"
        help="Size of the whole interface on this device"
        keywords="zoom"
        scope="device"
        testId="setting-row-ui-scale"
      >
        <Segmented
          label="UI scale"
          size="sm"
          value={String(uiScale)}
          onChange={(v) => setUiScale(Number(v) as UiScale)}
          items={SCALE_ITEMS}
        />
      </SettingRow>
    </Section>
  );
}
