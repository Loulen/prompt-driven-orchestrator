import { RotateCcw } from "lucide-react";
import { updateAppearance, useAppearance } from "../../../hooks/useAppearance";
import { deviationCount, resetToDefaults } from "../../../lib/appearance";
import { Button, Switch } from "../../../shared/ui/kit";
import type { SettingsSection } from "../../model/settingsCategories";
import AppearanceBlock from "./AppearanceBlock";
import { SettingRow } from "./fields";
import RunCanvasPreview from "./RunCanvasPreview";
import RunsListBlock from "./RunsListBlock";
import Section from "./Section";
import type { AppearanceEntities } from "./surfaceProps";

/**
 * Settings › Appearance › Appearance: one block per surface, each with its switches and a
 * preview (on the most recent entity the user sees, in `entities`), and « Reset to defaults · N » in the title bar (greyed while nothing differs).
 * Every switch writes the Actor's Apparence at once (`Your account`), out of the
 * instance form; rendered even when `GET /settings` failed.
 */
export default function AppearanceSection({
  section,
  entities,
}: {
  section: SettingsSection;
  entities?: AppearanceEntities;
}) {
  const appearance = useAppearance();
  const deviations = deviationCount(appearance);
  const { compactNodes } = appearance;
  const setCompactNodes = (on: boolean) => updateAppearance((current) => ({ ...current, compactNodes: on }));
  return (
    <Section
      section={section}
      tools={
        <Button
          size="sm"
          tone="ghost"
          icon={RotateCcw}
          disabled={deviations === 0}
          onClick={() => updateAppearance(resetToDefaults)}
          data-testid="appearance-reset"
          title={deviations === 0 ? "Everything is at its default" : "Put every choice of this section back to its default"}
        >
          {deviations === 0 ? "Reset to defaults" : `Reset to defaults · ${deviations}`}
        </Button>
      }
    >
      <RunsListBlock runs={entities?.runs ?? []} triggers={entities?.triggers ?? []} />
      <AppearanceBlock
        id="run-canvas"
        title="Run canvas"
        alwaysShown="Type icon, name, status, child runs of an orchestrator"
        note={compactNodes ? "Compact nodes is on: cards show their icon, name and status only." : undefined}
        preview={<RunCanvasPreview compact={compactNodes} />}
      >
        <SettingRow
          label="Compact nodes"
          help="Every card of a Run canvas on one line; the pipeline editor keeps them detailed"
          keywords="cards canvas"
          changed={compactNodes ? { default: "Off" } : undefined}
          testId="setting-row-compact-nodes"
        >
          <Switch label="Compact nodes" checked={compactNodes} onChange={setCompactNodes} />
        </SettingRow>
      </AppearanceBlock>
    </Section>
  );
}
