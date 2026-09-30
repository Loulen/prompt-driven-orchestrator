import { useMemo } from "react";
import type { Attributes } from "./attributes";
import { useContributions, useLocationContext } from "./catalogue";
import type { ListFilterContext, ListFilterProps } from "./contract";
import ExtensionBoundary from "./ExtensionBoundary";
import { LIST_FILTERS, type ListFilterLocation } from "./listFilters";

/**
 * A list-filter location (`runs/filter`, the 0.1 list-filter slot of the Runs;
 * `triggers/filter`, `pipelines/filter`, `projects/filter`): a row under the
 * list's own filters, one control per element. Nothing without a
 * contribution. The list keeps what carries every attribute of `filter`, as
 * `?attr.<id>/<key>=<value>` does on its endpoint.
 */
export default function ExtensionListFilters({
  location = "runs/filter",
  filter,
  onChange,
}: {
  location?: ListFilterLocation;
  filter: Attributes;
  onChange: (next: Attributes) => void;
}) {
  const resource = LIST_FILTERS[location];
  const extra = useMemo(() => ({ resource, filter }), [resource, filter]);
  const context = useLocationContext<ListFilterContext>(location, extra);
  const filters = useContributions<ListFilterProps>(location, context);
  if (filters.length === 0) return null;
  return (
    <div
      className="flex shrink-0 flex-wrap items-center gap-2 border-b border-line px-2 py-1.5 text-fg-3"
      style={{ fontSize: "10px" }}
      data-testid={`extension-filters-${resource}`}
    >
      {filters.map(({ key, extension, Component }) =>
        Component ? (
          <ExtensionBoundary key={key} extensionId={extension.id}>
            <Component
              extension={{ id: extension.id, name: extension.name }}
              context={context}
              resource={resource}
              filter={filter}
              setFilter={onChange}
            />
          </ExtensionBoundary>
        ) : null,
      )}
    </div>
  );
}
