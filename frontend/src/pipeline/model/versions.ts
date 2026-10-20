/**
 * The versions of a registry pipeline as the editor words them (ADR-0088):
 * a Save freezes the draft as `vN`, a Run retains the version it launched.
 */

import type { PipelineListEntry } from "../../shared/generated/PipelineListEntry";
import type { PipelineVersionRef } from "../../shared/generated/PipelineVersionRef";
import type { OpenPipeline } from "../store/editState";

/** A pipeline listed before it had versions is its file, v1. */
export const FIRST_VERSION: PipelineVersionRef = { number: 1, title: "v1" };

/** The last version of the listed pipeline `id`: the file. */
export function lastVersionOf(pipelines: PipelineListEntry[], id: string): PipelineVersionRef {
  return pipelines.find((p) => p.id === id)?.version ?? FIRST_VERSION;
}

/** The default title of the version `number`. */
export function defaultTitle(number: number): string {
  return `v${number}`;
}

/** « v7 · Add reviewer loop », or « v7 » when the title is the default one. */
export function versionLabel(version: PipelineVersionRef): string {
  const title = version.title.trim();
  return title === "" || title === defaultTitle(version.number)
    ? defaultTitle(version.number)
    : `${defaultTitle(version.number)} · ${title}`;
}

/** What a launch from the editor asks about: a draft that differs from the last version. */
export interface DraftLaunch {
  /** The tab whose draft would be saved. */
  tabId: string;
  pipelineName: string;
  /** The draft's changes since the last version. */
  changes: number;
  /** The number a « Save as vN and run » gives. */
  next: number;
  /** What « Run last version » launches. */
  last: PipelineVersionRef;
}

/**
 * The draft a launch of `pipelineId` would leave out, `null` when there is
 * none: no open tab of it, or a draft with no change since the last version.
 */
export function draftLaunch(
  pipelineId: string,
  tabs: OpenPipeline[],
  pipelines: PipelineListEntry[],
): DraftLaunch | null {
  const tab = tabs.find((t) => t.id === pipelineId && t.runId == null && t.version == null);
  const changes = tab?.draft?.changes ?? 0;
  if (!tab || changes === 0) return null;
  const last = lastVersionOf(pipelines, pipelineId);
  return { tabId: tab.id, pipelineName: tab.pipeline.name, changes, next: last.number + 1, last };
}

/** « 3 changes », « 1 change ». */
export function changesLabel(changes: number): string {
  return changes === 1 ? "1 change" : `${changes} changes`;
}

/** When a version was saved, as the history says it: « just now », « 12 min
 *  ago », « 2 h ago », « yesterday », else the day (« Oct 14 »). */
export function savedAgo(iso: string, now = new Date()): string {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return "";
  const s = Math.max(0, Math.round((now.getTime() - t) / 1000));
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  const then = new Date(t);
  const sameDay = then.toDateString() === now.toDateString();
  if (sameDay || s < 6 * 3600) return `${Math.round(s / 3600)} h ago`;
  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);
  if (then.toDateString() === yesterday.toDateString()) return "yesterday";
  return savedOn(iso);
}

/** The day a version was saved: « Oct 14 ». */
export function savedOn(iso: string): string {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return "";
  return new Date(t).toLocaleDateString("en-US", { month: "short", day: "numeric" });
}
