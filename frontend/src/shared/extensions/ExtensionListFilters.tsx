import type { Attributes } from "./attributes";
import ExtensionBoundary from "./ExtensionBoundary";
import { useListFilters, type ListFilterLocation } from "./listFilters";

/**
 * A list-filter location above a list (`triggers/filter`, `pipelines/filter`,
 * `projects/filter`): a row under the list's own filters, one control per
 * element. Nothing without a contribution. The Runs list puts its own
 * (`runs/filter`, `projects/filter`) in its filter menu instead.
 *
 * `projects/filter` sits above lists of another resource (the Triggers): its
 * row opens on a « Project » tag, so its controls never read as the list's own.
 */
export default function ExtensionListFilters({
  location,
  filter,
  onChange,
}: {
  location: ListFilterLocation;
  filter: Attributes;
  onChange: (next: Attributes) => void;
}) {
  const { resource, context, filters } = useListFilters(location, filter);
  if (filters.length === 0) return null;
  return (
    <div
      className="flex shrink-0 flex-wrap items-center gap-2 border-b border-line px-2 py-1.5 text-2xs text-text-3"
      data-testid={`extension-filters-${resource}`}
    >
      {location === "projects/filter" && (
        <span
          className="d-caps shrink-0 rounded border border-line-strong px-1 py-px text-text-3"
          title="These filters apply to the Project the items belong to"
          data-testid="extension-filters-project-tag"
        >
          Project
        </span>
      )}
      {filters.map(({ key, extension, contribution, Component }) =>
        Component ? (
          <ExtensionBoundary key={key} extensionId={extension.id}>
            <Component
              extension={{ id: extension.id, name: extension.name }}
              context={context}
              resource={resource}
              label={contribution.label}
              filter={filter}
              setFilter={onChange}
            />
          </ExtensionBoundary>
        ) : null,
      )}
    </div>
  );
}
