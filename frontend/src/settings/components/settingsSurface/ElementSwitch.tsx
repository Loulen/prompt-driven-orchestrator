import { useContext } from "react";
import { updateAppearance, useAppearance } from "../../../hooks/useAppearance";
import { ELEMENT_DEFAULTS, isChanged, isVisible, withElement } from "../../../lib/appearance";
import type { RowElement } from "../../../run/model/runRowElements";
import { Switch } from "../../../shared/ui/kit";
import { SettingRow } from "./fields";
import { searching, SearchContext, SectionMatchContext } from "./search";

/** A group's caps title in a block, left out under a search that did not find the section (its rows filter themselves). */
export function Group({ label, note }: { label: string; note?: string }) {
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
 * in its tooltip. `disabled` greys it (« Compact nodes » on): it keeps its value and
 * points nothing out.
 */
export function ElementSwitch({
  element,
  where,
  keywords,
  pointed,
  onPoint,
  disabled,
}: {
  element: RowElement;
  /** Where the element shows, for its help: « on every row of the Runs list ». */
  where: string;
  keywords: string;
  pointed: boolean;
  onPoint: (id: string | null) => void;
  disabled?: boolean;
}) {
  const appearance = useAppearance();
  const { id, label } = element;
  const shownByDefault = ELEMENT_DEFAULTS[id] ?? true;
  const point = (on: boolean) => onPoint(on && !disabled ? id : null);
  return (
    <div
      className="set-ablock__point"
      data-pointing={pointed || undefined}
      data-disabled={disabled || undefined}
      onPointerEnter={() => point(true)}
      onPointerLeave={() => point(false)}
      onFocus={() => point(true)}
      onBlur={() => point(false)}
    >
      <SettingRow
        label={label}
        help={`Show ${label} ${where}`}
        keywords={keywords}
        changed={isChanged(appearance, id) ? { default: shownByDefault ? "Shown" : "Hidden" } : undefined}
        testId={`setting-row-${id}`}
      >
        <Switch
          label={label}
          checked={isVisible(appearance, id)}
          disabled={disabled}
          onChange={(on) => updateAppearance((current) => withElement(current, id, on))}
        />
      </SettingRow>
    </div>
  );
}
