/**
 * The Éléments masquables of a Run's Info tab (surface `run.info`): what a
 * switch of Settings › Appearance hides. Its name, its status and its alert
 * banners (failure reason, awaiting reason, waiting for started nodes, source
 * fetch error, sandbox preparation) are fixed. The extensions' sections at
 * {@link RUN_INFO_LOCATIONS} are elements too, one each. A section whose every
 * element is hidden is left out.
 */
import type { ElementGroup } from "../../lib/appearance";
import type { UiLocation } from "../../shared/generated/UiLocation";

export const RUN_INFO = {
  note: "run.info.note",
  status: "run.info.status",
  harness: "run.info.harness",
  repo: "run.info.repo",
  sandbox: "run.info.sandbox",
  started: "run.info.started",
  trigger: "run.info.trigger",
  source: "run.info.source",
  duration: "run.info.duration",
  sessions: "run.info.sessions",
  changes: "run.info.changes",
  cost: "run.info.cost",
  graph: "run.info.graph",
} as const;

/** The switches of the Run info block, by group, in the tab's order. */
export const RUN_INFO_GROUPS: readonly ElementGroup[] = [
  { label: "Header", elements: [{ id: RUN_INFO.note, label: "Read-only note" }] },
  {
    label: "Run",
    elements: [
      { id: RUN_INFO.status, label: "Status" },
      { id: RUN_INFO.harness, label: "Harness" },
      { id: RUN_INFO.repo, label: "Repo" },
      { id: RUN_INFO.sandbox, label: "Sandbox" },
      { id: RUN_INFO.started, label: "Started" },
      { id: RUN_INFO.trigger, label: "Trigger" },
      { id: RUN_INFO.source, label: "Source" },
    ],
  },
  {
    label: "Stats",
    elements: [
      { id: RUN_INFO.duration, label: "Duration" },
      { id: RUN_INFO.sessions, label: "Node sessions" },
      { id: RUN_INFO.changes, label: "Changes" },
      { id: RUN_INFO.cost, label: "Est. cost" },
    ],
  },
  { label: "Pipeline", elements: [{ id: RUN_INFO.graph, label: "Graph" }] },
];

/** The extensions' location of a Run's Info tab: their sections. */
export const RUN_INFO_LOCATIONS: readonly UiLocation[] = ["run/info"];
