import { useState } from "react";
import { updateAppearance, useAppearance } from "../../../hooks/useAppearance";
import { RUN_CARD_ELEMENTS } from "../../../pipeline/model/runCardElements";
import { Switch } from "../../../shared/ui/kit";
import type { RecentRun } from "../../store/useRecentRun";
import AppearanceBlock from "./AppearanceBlock";
import { ElementSwitch } from "./ElementSwitch";
import { SettingRow } from "./fields";
import RunCanvasPreview from "./RunCanvasPreview";

/**
 * The Run canvas block of Settings › Appearance › Appearance: « Compact nodes » first,
 * the master switch, then a switch per element of a node's card, greyed while
 * « Compact nodes » is on; beside them the canvas's own cards for the most recent Run.
 */
export default function RunCanvasBlock({ recent, none }: { recent: RecentRun | null; none: boolean }) {
  const { compactNodes } = useAppearance();
  const [pointed, setPointed] = useState<string | null>(null);
  const setCompactNodes = (on: boolean) => updateAppearance((current) => ({ ...current, compactNodes: on }));
  return (
    <AppearanceBlock
      id="run-canvas"
      title="Run canvas"
      alwaysShown="Type icon, name, status, child runs of an orchestrator"
      note={compactNodes ? "Compact nodes is on: cards show their icon, name and status only." : undefined}
      preview={<RunCanvasPreview recent={recent} none={none} compact={compactNodes} pointed={compactNodes ? null : pointed} />}
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
      {RUN_CARD_ELEMENTS.map((element) => (
        <ElementSwitch
          key={element.id}
          element={element}
          where="on every card of a Run canvas; the pipeline editor shows everything"
          keywords="run canvas card node hide show"
          pointed={!compactNodes && pointed === element.id}
          onPoint={setPointed}
          disabled={compactNodes}
        />
      ))}
    </AppearanceBlock>
  );
}
