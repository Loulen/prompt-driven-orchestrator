/**
 * The Run section of a Run's Info tab: its fields (status, External source,
 * harness, repo, sandbox, start, Trigger), then its source.
 */

import { ExternalLink } from "lucide-react";
import type { RunState } from "../../shared/generated/RunState";
import type { SourceDrift } from "../../shared/generated/SourceDrift";
import { KeyValue, Section, StatusIcon, type KeyValueRow } from "../../shared/ui/kit";
import { useAppearancePreview, type ElementMarks } from "../../hooks/useElementMarks";
import { useSourceDrift } from "../../run/store/useSourceDrift";
import { followDeepLink, isDeepLink, isRunLink } from "../../shared/extensions/deepLink";
import { useTriggerName } from "../../trigger/store/useTriggerName";
import { RUN_INFO } from "../model/runInfoElements";
import { shortSavedAgo } from "../model/versions";
import { SourceBlock } from "./SourceBlock";

/** How an element renders: its marks, `null` when the Apparence hides it. */
export type Marks = (id: string) => ElementMarks | null;

/** `4m ago`, `3h ago`, `2d ago`: when the Run started. */
function startedAgo(iso: string): string {
  const ago = shortSavedAgo(iso);
  return ago === "now" || ago === "" ? ago || "—" : `${ago} ago`;
}

/** Whether the interface follows `link` (`#/ext/…`, `#/runs/…`): only then is it a link. */
const followable = (link: string) => isDeepLink(link) || isRunLink(link);

/** The drift a sample preview shows: no Run to read it from. */
const SAMPLE_DRIFT: SourceDrift = { state: "available", source_branch: "main", fork: "c2f81a0", ahead: 3, behind: 0 };

/**
 * The Run section: one row per field the Apparence shows and the Run has, then
 * its source. Nothing when no row is left and no source shows (hidden, or no
 * drift to read). A sample preview reads no drift: it shows its own.
 */
export function RunSection({ run, marks }: { run: RunState; marks: Marks }) {
  const sample = useAppearancePreview()?.sample === true;
  const read = useSourceDrift(sample ? null : run);
  const source = sample ? { ...read, drift: SAMPLE_DRIFT } : read;
  // The Trigger as the user named it; its id while unread or gone (CORE-232).
  const triggerName = useTriggerName(sample ? null : run.triggered_by);
  const row = (id: string, show: unknown, value: () => KeyValueRow): KeyValueRow[] => {
    const attrs = show ? marks(id) : null;
    return attrs ? [{ ...value(), attrs }] : [];
  };
  const rows = [
    ...row(RUN_INFO.status, true, () => ({
      k: "Status",
      v: (
        <span className="ei-line" data-testid="run-status">
          <StatusIcon kind="run" status={run.status} size="sm" />
          {run.status.replace(/_/g, " ")}
        </span>
      ),
      tip: run.status,
    })),
    // Where a Run an extension created comes from (its Provenance): its
    // label, a click follows its link, then the extension's id.
    ...row(RUN_INFO.externalSource, run.provenance, () => ({
      k: "External source",
      v: (
        <span className="ei-line" data-testid="run-external-source-info">
          {followable(run.provenance!.link) ? (
            <a
              className="ei-src"
              href={run.provenance!.link}
              onClick={(e) => {
                e.preventDefault();
                followDeepLink(run.provenance!.link);
              }}
            >
              {run.provenance!.label}
              <ExternalLink size={11} strokeWidth={1.5} aria-hidden />
            </a>
          ) : (
            <span>{run.provenance!.label}</span>
          )}
          <span className="d-meta">{run.provenance!.extension}</span>
        </span>
      ),
      tip: `${run.provenance!.label} (${run.provenance!.extension}) · ${run.provenance!.link}`,
    })),
    ...row(RUN_INFO.harness, run.harness, () => ({
      k: "Harness",
      v: <span data-testid="run-harness">{run.harness}</span>,
      mono: true,
      tip: run.harness,
    })),
    ...row(RUN_INFO.repo, run.target_repo, () => ({ k: "Repo", v: run.target_repo, mono: true })),
    // Sandbox (#410): any sandboxed Run (full/minimal); an `off`/host Run has no row.
    ...row(RUN_INFO.sandbox, run.sandbox && run.sandbox !== "off", () => ({
      k: "Sandbox",
      v: <span data-testid="sandbox-badge">{run.sandbox}</span>,
      mono: true,
      tip: `Docker sandbox (${run.sandbox})`,
    })),
    ...row(RUN_INFO.started, run.started_at, () => ({
      k: "Started",
      v: <span data-testid="run-started">{startedAgo(run.started_at!)}</span>,
      tip: Number.isNaN(Date.parse(run.started_at!)) ? run.started_at! : new Date(run.started_at!).toLocaleString(),
    })),
    ...row(RUN_INFO.trigger, run.triggered_by, () => ({
      k: "Trigger",
      v: <span data-testid="run-trigger">{triggerName ?? run.triggered_by}</span>,
      mono: triggerName === null,
      tip: run.triggered_by ?? undefined,
    })),
  ];
  const sourceMarks = source.drift ? marks(RUN_INFO.source) : null;
  if (rows.length === 0 && !sourceMarks) return null;
  return (
    <Section title="Run">
      {rows.length > 0 && <KeyValue rows={rows} />}
      {sourceMarks && <SourceBlock run={run} source={source} marks={sourceMarks} />}
    </Section>
  );
}
