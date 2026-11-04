import { useMemo } from "react";
import { AppearancePreviewContext } from "../../../hooks/useElementMarks";
import TriggerRow from "../../../trigger/components/triggersList/TriggerRow";
import type { TriggerListEntry } from "../../../shared/generated/TriggerListEntry";
import { triggersListPreview } from "../../model/triggersListPreview";

const nothing = () => {};

/**
 * The preview of the Triggers list block: the list's own rows (`TriggerRow`) on the
 * Triggers the user sees that fired last, or on a sample Trigger without extension
 * marks, at the list's width. Inert: nothing in it reacts. `pointed` is the element
 * whose switch the pointer or the focus is on.
 */
export default function TriggersListPreview({
  triggers,
  pointed,
}: {
  triggers: TriggerListEntry[];
  pointed: string | null;
}) {
  const shown = useMemo(() => triggersListPreview(triggers), [triggers]);
  const context = useMemo(() => ({ pointed, sample: shown.sample }), [pointed, shown.sample]);
  return (
    <AppearancePreviewContext.Provider value={context}>
      <figure className="set-preview" data-testid="appearance-preview-triggers-list" data-sample={shown.sample || undefined}>
        <div className="lp-pane set-preview__list" inert>
          <div className="lp-rows">
            {shown.triggers.map((trigger) => (
              <TriggerRow
                key={trigger.id}
                trigger={trigger}
                isSelected={false}
                rowSelected={false}
                onSelect={nothing}
                onSelectControl={nothing}
                onDelete={nothing}
                onToggle={nothing}
              />
            ))}
          </div>
        </div>
        <figcaption className="set-preview__caption">{shown.caption}</figcaption>
      </figure>
    </AppearancePreviewContext.Provider>
  );
}
