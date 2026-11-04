import { useContext } from "react";
import { updateAppearance, useAppearance } from "../../../hooks/useAppearance";
import { ELEMENT_DEFAULTS, isChanged, isVisible, withElement } from "../../../lib/appearance";
import type { RowElement } from "../../../run/model/runRowElements";
import { Switch } from "../../../shared/ui/kit";
import { SettingRow } from "./fields";
import { searching, SearchContext, SectionMatchContext } from "./search";

/** A group's caps title, left out under a search that did not find the section (its rows filter themselves). */
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
 * in its tooltip.
 */
export function ElementSwitch({
  element,
  surfaceName,
  pointed,
  onPoint,
}: {
  element: RowElement;
  /** The list it hides the element of, in its help (« Runs list »). */
  surfaceName: string;
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
        help={`Show ${label} on every row of the ${surfaceName}`}
        keywords={`${surfaceName.toLowerCase()} row hide show`}
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
