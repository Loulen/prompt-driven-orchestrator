import { Fragment } from "react";
import { ArrowUp, Layers } from "lucide-react";
import { IconButton } from "../../../../shared/ui/kit";

/**
 * The drill path of a tab, on the left of its filter bar
 * (`overlays.statsFilters`): up a level, then `Total / pipeline / node`, every
 * crumb but the last a button back to its level.
 */
export function Breadcrumb({
  crumbs,
  testid = "stats-cost-breadcrumb",
}: {
  crumbs: { label: string; onClick?: () => void }[];
  /** The Performance and Sessions tabs reuse it under their own test id. */
  testid?: string;
}) {
  const up = crumbs.length > 1 ? crumbs[crumbs.length - 2].onClick : undefined;
  return (
    <nav className="sx-crumbs" aria-label="Level" data-testid={testid}>
      <IconButton icon={ArrowUp} label="Up a level" size="sm" disabled={!up} onClick={up} />
      {crumbs.map((crumb, index) => (
        <Fragment key={`${crumb.label}-${index}`}>
          {index > 0 ? (
            <span className="sx-crumbs__sep" aria-hidden>
              {" / "}
            </span>
          ) : null}
          {crumb.onClick ? (
            <button
              type="button"
              aria-label={`Back to ${crumb.label}`}
              title={crumb.label}
              onClick={crumb.onClick}
              className="sx-crumb"
            >
              {index === 0 && <Layers size={13} strokeWidth={1.5} aria-hidden />}
              <span>{crumb.label}</span>
            </button>
          ) : (
            <span className="sx-crumb" aria-current="page" title={crumb.label}>
              {index === 0 && <Layers size={13} strokeWidth={1.5} aria-hidden />}
              <span>{crumb.label}</span>
            </span>
          )}
        </Fragment>
      ))}
    </nav>
  );
}
