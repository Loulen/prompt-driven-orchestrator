import { useContext, type ReactNode } from "react";
import { HeadlessBlockContext, searching, SearchContext, SectionMatchContext } from "./search";

/**
 * One surface's block of Settings › Appearance › Appearance: its name and what it always
 * shows in the header, its switches on the left (setting rows), a preview of the surface
 * beside them. Under a search that did not find the section, only its rows show, so the
 * search finds them like any other row.
 */
export default function AppearanceBlock({
  id,
  title,
  alwaysShown,
  note,
  preview,
  children,
}: {
  /** The block's test id suffix (`run-canvas`…). */
  id: string;
  title: string;
  /** Its fixed elements, never hidden. */
  alwaysShown: string;
  /** One line under the switches. */
  note?: ReactNode;
  preview?: ReactNode;
  children: ReactNode;
}) {
  const query = useContext(SearchContext);
  const labelMatch = useContext(SectionMatchContext);
  if (searching(query) && !labelMatch) {
    return <HeadlessBlockContext.Provider value={title}>{children}</HeadlessBlockContext.Provider>;
  }
  return (
    <div className="set-ablock" data-testid={`appearance-block-${id}`}>
      <header className="set-ablock__head">
        <h4 className="set-ablock__title">{title}</h4>
        <span className="set-ablock__fixed" title={`Always shown: ${alwaysShown}`}>
          Always shown: {alwaysShown}
        </span>
      </header>
      <div className="set-ablock__body">
        <div className="set-ablock__switches">
          {children}
          {note && <p className="set-ablock__note">{note}</p>}
        </div>
        {preview && <div className="set-ablock__preview">{preview}</div>}
      </div>
    </div>
  );
}
