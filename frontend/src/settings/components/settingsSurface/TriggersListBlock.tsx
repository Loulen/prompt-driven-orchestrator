import { useState } from "react";
import { TRIGGER_ROW_ELEMENTS, TRIGGER_ROW_LOCATIONS } from "../../../trigger/model/triggerRowElements";
import type { RowElement } from "../../../lib/appearance";
import { useHideableContributions } from "../../../shared/extensions/hideableContributions";
import type { TriggerListEntry } from "../../../shared/generated/TriggerListEntry";
import AppearanceBlock from "./AppearanceBlock";
import { ElementSwitch, Group } from "./ElementSwitch";
import TriggersListPreview from "./TriggersListPreview";

/**
 * The Triggers list block of Settings › Appearance › Appearance: a switch per element
 * of a row, then one per extension contribution (« Extension · label ») under
 * *Extensions*, and beside them the list's own rows as a preview.
 */
export default function TriggersListBlock({ triggers }: { triggers: TriggerListEntry[] }) {
  const contributions = useHideableContributions("triggers.list", TRIGGER_ROW_LOCATIONS);
  const [pointed, setPointed] = useState<string | null>(null);
  const toggle = (element: RowElement) => (
    <ElementSwitch
      key={element.id}
      element={element}
      where="on every row of the Triggers list"
      keywords="triggers list row hide show"
      pointed={pointed === element.id}
      onPoint={setPointed}
    />
  );
  return (
    <AppearanceBlock
      id="triggers-list"
      title="Triggers list"
      alwaysShown="Name, enable switch, last result"
      preview={<TriggersListPreview triggers={triggers} pointed={pointed} />}
    >
      {TRIGGER_ROW_ELEMENTS.map(toggle)}
      {contributions.length > 0 && <Group label="Extensions" />}
      {contributions.map(toggle)}
    </AppearanceBlock>
  );
}
