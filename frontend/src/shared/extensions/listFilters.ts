/** The list-filter locations, and the resource each filters. */
export const LIST_FILTERS = {
  "runs/filter": "run",
  "triggers/filter": "trigger",
  "pipelines/filter": "pipeline",
  "projects/filter": "project",
} as const;

export type ListFilterLocation = keyof typeof LIST_FILTERS;

