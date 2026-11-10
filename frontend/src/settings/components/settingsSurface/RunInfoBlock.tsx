import { useState } from "react";
import { RUN_INFO_GROUPS, RUN_INFO_LOCATIONS } from "../../../pipeline/model/runInfoElements";
import type { RowElement } from "../../../lib/appearance";
import { useHideableContributions } from "../../../shared/extensions/hideableContributions";
import type { RecentRun } from "../../store/useRecentRun";
import AppearanceBlock from "./AppearanceBlock";
import { ElementSwitch, Group } from "./ElementSwitch";
import RunInfoPreview from "./RunInfoPreview";

/**
 * The Run info tab block of Settings › Appearance › Appearance: a switch per field of
 * a Run's Info tab, by group (Header, Run, Stats, Pipeline), then one per extension
 * section (« Extension · label »); beside them the tab itself on the most recent Run.
 */
export default function RunInfoBlock({ recent, none }: { recent: RecentRun | null; none: boolean }) {
  const contributions = useHideableContributions("run.info", RUN_INFO_LOCATIONS);
  const [pointed, setPointed] = useState<string | null>(null);
  const toggle = (element: RowElement) => (
    <ElementSwitch
      key={element.id}
      element={element}
      where="in the Info tab of a Run"
      keywords="run info tab field section hide show"
      pointed={pointed === element.id}
      onPoint={setPointed}
    />
  );
  return (
    <AppearanceBlock
      id="run-info"
      title="Run info tab"
      alwaysShown="Name, status, alert banners"
      preview={<RunInfoPreview recent={recent} none={none} pointed={pointed} />}
    >
      {RUN_INFO_GROUPS.map((group) => [<Group key={group.label} label={group.label} />, ...group.elements.map(toggle)])}
      {contributions.length > 0 && <Group label="Extension sections" />}
      {contributions.map(toggle)}
    </AppearanceBlock>
  );
}
