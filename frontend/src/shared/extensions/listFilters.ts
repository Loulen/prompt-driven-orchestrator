import { useMemo } from "react";
import type { Attributes } from "./attributes";
import { useContributions, useLocationContext } from "./catalogue";
import type { ListFilterContext, ListFilterProps } from "./contract";

/** The list-filter locations, and the resource each filters. */
export const LIST_FILTERS = {
  "runs/filter": "run",
  "triggers/filter": "trigger",
  "pipelines/filter": "pipeline",
  "projects/filter": "project",
} as const;

export type ListFilterLocation = keyof typeof LIST_FILTERS;

/**
 * The list filters contributed at `location`, the context they render with
 * and the resource they keep. The list keeps what carries every attribute of
 * `filter`, as `?attr.<id>/<key>=<value>` does on its endpoint.
 */
export function useListFilters(location: ListFilterLocation, filter: Attributes) {
  const resource = LIST_FILTERS[location];
  const extra = useMemo(() => ({ resource, filter }), [resource, filter]);
  const context = useLocationContext<ListFilterContext>(location, extra);
  const filters = useContributions<ListFilterProps>(location, context);
  return { resource, context, filters };
}
