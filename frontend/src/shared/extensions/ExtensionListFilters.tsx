import { useMemo } from "react";
import type { Attributes } from "./attributes";
import { useContributions, useLocationContext } from "./catalogue";
import type { ListFilterContext, ListFilterProps } from "./contract";
import ExtensionBoundary from "./ExtensionBoundary";

/**
 * The `runs/filter` location (the 0.1 list-filter slot of the Runs): a row
 * under the list's own filters, one control per element. Nothing without a
 * contribution.
 */
export default function ExtensionListFilters({
  filter,
  onChange,
}: {
  filter: Attributes;
  onChange: (next: Attributes) => void;
}) {
  const resource = "run";
  const extra = useMemo(() => ({ resource, filter }), [filter]);
  const context = useLocationContext<ListFilterContext>("runs/filter", extra);
  const filters = useContributions<ListFilterProps>("runs/filter", context);
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
