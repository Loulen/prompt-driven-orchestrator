import { RotateCcw } from "lucide-react";
import { updateAppearance, useAppearance } from "../../../hooks/useAppearance";
import { deviationCount, resetToDefaults } from "../../../lib/appearance";
import { Button } from "../../../shared/ui/kit";
import type { SettingsSection } from "../../model/settingsCategories";
import { useRecentRun } from "../../store/useRecentRun";
import PipelinesListBlock from "./PipelinesListBlock";
import RunCanvasBlock from "./RunCanvasBlock";
import RunInfoBlock from "./RunInfoBlock";
import RunsListBlock from "./RunsListBlock";
import Section from "./Section";
import TriggersListBlock from "./TriggersListBlock";
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
  const runs = entities?.runs ?? [];
  const { recent, none } = useRecentRun(runs);
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
      <RunsListBlock runs={runs} triggers={entities?.triggers ?? []} />
      <RunCanvasBlock recent={recent} none={none} />
      <RunInfoBlock recent={recent} none={none} />
      <TriggersListBlock triggers={entities?.triggers ?? []} />
      <PipelinesListBlock projects={entities?.projects ?? []} />
    </Section>
  );
}
