import { useContext, useState } from "react";
import { updateAppearance, useAppearance } from "../../../hooks/useAppearance";
import { ELEMENT_DEFAULTS, isChanged, isVisible, withElement } from "../../../lib/appearance";
import { RUN_ROW_FIRST_LINE, RUN_ROW_LOCATIONS, RUN_ROW_SECOND_LINE, type RowElement } from "../../../run/model/runRowElements";
import { useHideableContributions } from "../../../shared/extensions/hideableContributions";
import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import type { TriggerListEntry } from "../../../shared/generated/TriggerListEntry";
import { Switch } from "../../../shared/ui/kit";
import AppearanceBlock from "./AppearanceBlock";
import { SettingRow } from "./fields";
import RunsListPreview from "./RunsListPreview";
import { searching, SearchContext, SectionMatchContext } from "./search";

/** A group's caps title, left out under a search that did not find the section (its rows filter themselves). */
function Group({ label, note }: { label: string; note?: string }) {
  const query = useContext(SearchContext);
  const sectionFound = useContext(SectionMatchContext);
  if (searching(query) && !sectionFound) return null;
  return (
    <h5 className="set-ablock__group">
      {label}
      {note && <span className="set-ablock__group-note">{note}</span>}
    </h5>
  );
}

/**
 * One Élément masquable's switch: hovering or focusing it points its element out in
 * the preview. A choice that differs from the default says « changed », the default
 * in its tooltip.
 */
function ElementSwitch({
  element,
  pointed,
  onPoint,
}: {
  element: RowElement;
  pointed: boolean;
  onPoint: (id: string | null) => void;
}) {
  const appearance = useAppearance();
  const { id, label } = element;
  const shownByDefault = ELEMENT_DEFAULTS[id] ?? true;
  return (
    <div
      className="set-ablock__point"
      data-pointing={pointed || undefined}
      onPointerEnter={() => onPoint(id)}
      onPointerLeave={() => onPoint(null)}
      onFocus={() => onPoint(id)}
      onBlur={() => onPoint(null)}
    >
      <SettingRow
        label={label}
        help={`Show ${label} on every row of the Runs list`}
        keywords="runs list row hide show"
        changed={isChanged(appearance, id) ? { default: shownByDefault ? "Shown" : "Hidden" } : undefined}
        testId={`setting-row-${id}`}
      >
        <Switch
          label={label}
          checked={isVisible(appearance, id)}
          onChange={(on) => updateAppearance((current) => withElement(current, id, on))}
        />
      </SettingRow>
    </div>
  );
}

/**
 * The Runs list block of Settings › Appearance › Appearance: a switch per element of a
 * row, first line then second line (the core's, then each extension contribution,
 * « Extension · label »), and beside them the list's own rows as a preview.
 */
export default function RunsListBlock({ runs, triggers }: { runs: RunListEntry[]; triggers: TriggerListEntry[] }) {
  const contributions = useHideableContributions("runs.list", RUN_ROW_LOCATIONS);
  const [pointed, setPointed] = useState<string | null>(null);
  const toggle = (element: RowElement) => (
    <ElementSwitch key={element.id} element={element} pointed={pointed === element.id} onPoint={setPointed} />
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
