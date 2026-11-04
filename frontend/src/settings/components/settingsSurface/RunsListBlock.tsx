import { useState } from "react";
import { RUN_ROW_FIRST_LINE, RUN_ROW_LOCATIONS, RUN_ROW_SECOND_LINE, type RowElement } from "../../../run/model/runRowElements";
import { useHideableContributions } from "../../../shared/extensions/hideableContributions";
import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import type { TriggerListEntry } from "../../../shared/generated/TriggerListEntry";
import AppearanceBlock from "./AppearanceBlock";
import { ElementSwitch, Group } from "./ElementSwitch";
import RunsListPreview from "./RunsListPreview";

/**
 * The Runs list block of Settings › Appearance › Appearance: a switch per element of a
 * row, first line then second line (the core's, then each extension contribution,
 * « Extension · label »), and beside them the list's own rows as a preview.
 */
export default function RunsListBlock({ runs, triggers }: { runs: RunListEntry[]; triggers: TriggerListEntry[] }) {
  const contributions = useHideableContributions("runs.list", RUN_ROW_LOCATIONS);
  const [pointed, setPointed] = useState<string | null>(null);
  const toggle = (element: RowElement) => (
    <ElementSwitch
      key={element.id}
      element={element}
      surfaceName="Runs list"
      pointed={pointed === element.id}
      onPoint={setPointed}
    />
  );
  return (
    <AppearanceBlock
      id="runs-list"
      title="Runs list"
      alwaysShown="Status, name"
      preview={<RunsListPreview runs={runs} triggers={triggers} pointed={pointed} />}
    >
      <Group label="First line" />
      {RUN_ROW_FIRST_LINE.map(toggle)}
      <Group label="Second line" note="All hidden: rows take one line" />
      {RUN_ROW_SECOND_LINE.map(toggle)}
      {contributions.map(toggle)}
    </AppearanceBlock>
  );
}
