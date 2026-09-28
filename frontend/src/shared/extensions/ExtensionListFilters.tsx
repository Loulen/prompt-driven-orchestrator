import type { Attributes } from "./attributes";
import ExtensionBoundary from "./ExtensionBoundary";
import { useSlot, type ListFilterProps } from "./slots";

/**
 * The list-filter slot of the list of `resource`: a row under the list's own
 * filters, one control per extension element. Nothing without a contribution.
 */
export default function ExtensionListFilters({
  resource,
  filter,
  onChange,
}: {
  resource: string;
  filter: Attributes;
  onChange: (next: Attributes) => void;
}) {
  const filters = useSlot<ListFilterProps>("list_filter", resource);
  if (filters.length === 0) return null;
  return (
    <div
      className="flex shrink-0 flex-wrap items-center gap-2 border-b border-line px-2 py-1.5 text-fg-3"
      style={{ fontSize: "10px" }}
      data-testid={`extension-filters-${resource}`}
    >
      {filters.map(({ key, extension, Component }) => (
        <ExtensionBoundary key={key} extensionId={extension.id}>
          <Component
            extension={{ id: extension.id, name: extension.name }}
            resource={resource}
            filter={filter}
            setFilter={onChange}
          />
        </ExtensionBoundary>
      ))}
    </div>
  );
}
