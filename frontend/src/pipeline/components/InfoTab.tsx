/**
 * The Info tab: a pipeline's metadata (editable on a template, read-only on a
 * Run), a Run's status, source and stats.
 */

import { Info, Box, Loader, CloudOff } from "lucide-react";
import { WaitingForStartedNodesReason } from "../../run/components/WaitingForStartedNodesBanner";
import { SectionHead } from "./InspectorPrimitives";
import type { RunState } from "../../shared/generated/RunState";
import { isLiveRun } from "../../run/model/runStatus";
import { formatDuration, useRunDuration } from "../../run/model/runDuration";
import { formatEstCost } from "../../run/model/costLabel";
import type { EditorPipelineDef } from "../model/pipeline";

import { SourceBlock } from "./SourceBlock";
import PipelineMetaSections from "./PipelineMetaSections";
import { PipelineConfigSections } from "./ExtensionInspectorSections";
import { useEditStore } from "../store/editStore";
import { InfoVersionLine, NewerVersionPill } from "./InfoVersion";
import { useMemo } from "react";
import ExtensionSections, { type RenderedSection } from "../../shared/extensions/ExtensionSections";
import type { RunInfoContext } from "../../shared/extensions/contract";

function infoSection({ key, label, body }: RenderedSection) {
  return (
    <div className="border-b border-line px-3 py-3" style={{ fontSize: "11.5px" }} data-testid={`extension-section-${key}`}>
      <SectionHead title={label} />
      <div className="mt-2">{body}</div>
    </div>
  );
}

/** The `run/info` sections of the extensions (ADR-0081, #31), after the Run's stats. */
function RunInfoSections({ run }: { run: RunState }) {
  const extra = useMemo(
    () => ({
      entity: {
        kind: "run",
        id: run.run_id,
        attributes: run.attributes ?? {},
        ...(run.name ? { name: run.name } : {}),
        created_by: run.created_by ?? null,
        trigger_editor: run.trigger_editor ?? null,
      },
    }),
    [run.run_id, run.attributes, run.name, run.created_by, run.trigger_editor],
  );
  return <ExtensionSections<RunInfoContext> location="run/info" extra={extra} wrap={infoSection} />;
}

function StatRow({
  label,
  children,
  testid,
}: {
  label: string;
  children: React.ReactNode;
  testid?: string;
}) {
  return (
    <div
      className="flex items-center justify-between rounded bg-bg-3 px-2 py-1"
      style={{ fontSize: "10.5px" }}
      data-testid={testid}
    >
      <span className="text-fg-3">{label}</span>
      <span className="font-mono text-fg-4">{children}</span>
    </div>
  );
}

const STATUS_DOT: Record<string, string> = {
  running: "bg-st-running animate-pulse",
  awaiting_user: "bg-st-await",
  completed: "bg-st-done",
  failed: "bg-st-failed",
  halted: "bg-st-blocked",
  archived: "bg-st-archived",
  pending: "bg-st-pending",
};

export function InfoTab({
  run,
  pipeline,
  pipelineName,
  pipelineId,
  hasAssistant,
  onOpenDiff,
}: {
  run: RunState | null;
  pipeline: EditorPipelineDef | null;
  pipelineName: string;
  /** The template's id (its edit tab), for the `pipeline/config` sections; null on a Run. */
  pipelineId: string | null;
  hasAssistant: boolean;
  /** #748: the Changes stat is the link to what it counts — the Diff tab. */
  onOpenDiff: () => void;
}) {
  const durationMs = useRunDuration(run?.started_at, run?.completed_at, run?.status);
  const durationLabel = formatDuration(durationMs);
  const durationTicking = run != null && run.completed_at == null && isLiveRun(run.status);
  const pipelines = useEditStore((s) => s.pipelines);
  // ADR-0088: a version shown in its own tab is read only, like a Run.
  const viewingVersion = useEditStore(
    (s) => s.openTabs.find((t) => t.id === s.activeTabId)?.version != null,
  );

  return (
    <>
      <div className="border-b border-line px-3 py-3" style={{ fontSize: "11.5px" }}>
        <div className="flex items-center gap-2">
          <span
            className={`h-2 w-2 shrink-0 rounded-full ${
              STATUS_DOT[run?.status ?? ""] ?? "bg-st-pending"
            }`}
          />
          <div className="min-w-0 flex-1">
            <div className="font-medium text-fg" data-testid="info-panel-name">
              {pipelineName}
            </div>
            <InfoVersionLine run={run} pipelineId={pipelineId} />
          </div>
          <NewerVersionPill run={run} />
          {/* Sandbox badge (#410): shown for any sandboxed Run (full/minimal). An
              `off`/host Run renders nothing — the field is absent on those. */}
          {run?.sandbox && run.sandbox !== "off" && (
            <span
              className="flex shrink-0 items-center gap-1 rounded bg-bg-3 px-1.5 py-0.5 font-mono text-fg-3"
              style={{ fontSize: "9.5px" }}
              data-testid="sandbox-badge"
              title={`This run is isolated in a Docker sandbox (${run.sandbox})`}
            >
              <Box size={10} className="shrink-0" />
              sandbox: {run.sandbox}
            </span>
          )}
        </div>

        {/* #752: what the standalone Run-info sidebar carried besides Repositories
            lives here now — the failure / awaiting reason (#503, #598), the frozen
            harness (#551) and the editing note (#315). Clicking a red dot lands on
            this tab, so the failure must be the first thing it says. */}
        {run?.failure_reason && (
          <div
            className="mt-2 rounded border border-st-failed/30 bg-st-failed-bg px-2 py-1.5 text-fg-2"
            style={{ fontSize: "10.5px" }}
            data-testid="run-failure-reason"
          >
            <div className="font-medium text-st-failed">
              {run.status === "halted" ? "Halted" : run.status === "skipped" ? "Skipped" : "Failed"}
            </div>
            <div className="mt-0.5 break-words">{run.failure_reason}</div>
          </div>
        )}
        {run?.awaiting_reason && (
          <div
            className="mt-2 rounded border border-st-await/30 bg-st-await-bg px-2 py-1.5 text-fg-2"
            style={{ fontSize: "10.5px" }}
            data-testid="run-awaiting-reason"
          >
            {/* #588: only an incident carries a machine slug; a declared wait is
                the agent's question (or an awaiting child), not an interruption. */}
            <div className="font-medium text-st-await">
              {run.awaiting_reason_code ? "Interrupted · awaiting you" : "Awaiting you"}
            </div>
            <div className="mt-0.5 break-words">{run.awaiting_reason}</div>
          </div>
        )}
        <WaitingForStartedNodesReason run={run ?? null} />
        {/* #804 / ADR-0070 §1: a Trigger fire that could not refresh the remotes
            before cutting. Deliberately NOT red and NOT a failure — this Run ran;
            it simply started from the last state known locally, which is the one
            thing you cannot deduce from anywhere else on the page. Stale-tinted
            (the same amber as a stale node), between the awaiting box and the
            harness: it explains what the Run started ON, so it belongs with the
            other facts about its start. */}
        {run?.source_fetch_error && (
          <div
            className="mt-2 rounded border border-st-stale/30 bg-st-stale-bg px-2 py-1.5 text-fg-2"
            style={{ fontSize: "10.5px" }}
            data-testid="run-source-fetch-error"
          >
            <div className="flex items-center gap-1.5 font-medium text-st-stale">
              <CloudOff size={11} className="shrink-0" />
              Started on local state · fetch failed
            </div>
            <div className="mt-0.5 break-words">
              {run.source_branch ? (
                <>
                  Source branch <span className="font-mono">{run.source_branch}</span> could not
                  be refreshed before the cut.
                </>
              ) : (
                <>The source ref could not be refreshed before the cut.</>
              )}{" "}
              The Run starts from the last state known locally.
            </div>
            {/* git's own words, one line: enough to recognise "no network" from
                "no key" at a glance, the whole of it on hover and selectable for
                a paste into a terminal. */}
            <div
              className="mt-1 truncate font-mono text-fg-4"
              style={{ fontSize: "9.5px" }}
              title={run.source_fetch_error.message}
              data-testid="run-source-fetch-stderr"
            >
              {run.source_fetch_error.message}
            </div>
          </div>
        )}
        {run?.harness && (
          <div className="mt-2 flex items-center gap-1.5 text-fg-3" style={{ fontSize: "10.5px" }} data-testid="run-harness">
            <span className="text-fg-4">Harness</span>
            <span className="rounded bg-bg-3 px-1.5 py-0.5 font-mono text-fg-2">{run.harness}</span>
          </div>
        )}
        {run && <SourceBlock run={run} />}
        {run && (
          <div
            className="mt-2 rounded border border-line-strong bg-bg-3 px-2 py-1.5 text-fg-3"
            style={{ fontSize: "10.5px" }}
            data-testid="run-info-note"
          >
            {run.status === "archived"
              ? "Archived run · read-only · outputs preserved"
              : "Canvas edits apply to this run only · run settings are read-only"}
          </div>
        )}
      </div>

      {/* Sandbox preparation banner (#410, amber): the image is being pulled/built
          at first use. Only while `sandbox_prep === "pending"` — it clears to
          `ready` once the container is about to run, so the Run never looks stuck. */}
      {run?.sandbox_prep === "pending" && (
        <div
          className="flex items-center gap-2 border-b border-st-await/30 bg-st-await-bg px-3 py-2"
          data-testid="sandbox-prep-banner"
        >
          <Loader size={14} className="shrink-0 animate-spin text-st-await" />
          <span
            className="text-st-await"
            style={{ fontSize: "11.5px", fontWeight: 500 }}
          >
            Preparing the sandbox — pulling/building the image…
          </span>
        </div>
      )}

      {/* Notion #6 / #948: Pipeline info is the only surface of the pipeline's
          metadata — editable on a template, read-only on a Run (active or
          archived) and on a version. Order: header → Identity → Variables → Canvas → Stats → note. */}
      {pipeline && <PipelineMetaSections pipeline={pipeline} readOnly={run != null || viewingVersion} />}

      {(run || pipeline) && (
        <div
          className="border-b border-line px-3 py-3"
          style={{ fontSize: "11.5px" }}
          data-testid="info-stats"
        >
          <SectionHead title="Stats" />
          {pipeline && (
            <div className="mt-2 flex gap-4 text-fg-4" style={{ fontSize: "10px" }} data-testid="pipeline-graph-stats">
              <span>{pipeline.nodes.length} nodes</span>
              <span>{pipeline.edges.length} edges</span>
            </div>
          )}
          {run && (
          <div className="mt-2 flex flex-col gap-1" data-testid="run-stats">
            <StatRow label="Duration" testid="stat-duration">
              <span className="flex items-center gap-1.5">
                {durationLabel ?? "—"}
                {durationTicking && (
                  <span
                    className="h-1.5 w-1.5 rounded-full bg-st-running animate-pulse"
                    title="live"
                    data-testid="stat-duration-live"
                  />
                )}
              </span>
            </StatRow>
            <StatRow label="Node sessions started" testid="stat-sessions">
              {(run.sessions_spawned ?? 0).toLocaleString()}
            </StatRow>
            <StatRow label="Changes" testid="stat-loc">
              {run.loc ? (
                // #748: the LOC stat opens the Diff tab — the count IS what the
                // tab shows (same endpoint bounds), so the number links to it.
                <button
                  onClick={onOpenDiff}
                  className="flex items-center gap-1.5 rounded px-1 -mx-1 transition-colors hover:bg-bg-4 hover:text-fg-2 cursor-pointer"
                  data-testid="stat-loc-open-diff"
                  title="Open the Diff tab"
                >
                  <span className="text-st-done">
                    +{run.loc.insertions.toLocaleString()}
                  </span>
                  <span className="text-st-failed">
                    −{run.loc.deletions.toLocaleString()}
                  </span>
                  <span className="text-fg-4">
                    · {run.loc.files_changed.toLocaleString()}{" "}
                    {run.loc.files_changed === 1 ? "file" : "files"}
                  </span>
                  <span className="text-fg-4" aria-hidden>↗</span>
                </button>
              ) : (
                "—"
              )}
            </StatRow>
            <StatRow label="Est. cost" testid="stat-cost">
              {run.cost ? (
                (() => {
                  // Shared honesty helper (#272/#377): same vocabulary as the
                  // aggregated Stats charts.
                  const c = formatEstCost(
                    run.cost.usd,
                    run.cost.partial,
                    run.cost.unpriced_models,
                    // #553: "—" + reason when a node ran on a harness with no cost
                    // source (e.g. opencode) — never a misleading $0.
                    run.cost.uncosted_harnesses ?? [],
                    // #615: ventilate a mixed Run's total by harness.
                    run.cost.by_harness ?? [],
                  );
                  // Show the per-harness breakdown only when it says more than the
                  // total already does: a genuinely mixed Run (≥2 slices), or — when
                  // the total is withheld as "—" — any slice at all, since then even
                  // one says something the total cannot (#617 FP).
                  const slices = c.ventilation ?? [];
                  const ventilated = slices.length > (c.text === "—" ? 0 : 1);
                  return (
                    <span className="flex flex-col items-end gap-0.5" title={c.title}>
                      <span className="flex items-center gap-1">
                        {c.text}
                        {c.dagger && <span className="text-st-await">†</span>}
                      </span>
                      {ventilated && (
                        <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
                          {slices
                            .map((v) => `${v.text} via ${v.harness}`)
                            .join(" · ")}
                        </span>
                      )}
                    </span>
                  );
                })()
              ) : (
                "—"
              )}
            </StatRow>
          </div>
          )}
        </div>
      )}

      {run && <RunInfoSections run={run} />}

      {/* ADR-0081 (#31): the `pipeline/config` sections of the extensions, after
          a template's Stats — they followed the standalone inspector here (#948). */}
      {!run && pipeline && pipelineId && (
        <div className="flex flex-col gap-3 border-b border-line px-3 py-3 empty:hidden" style={{ fontSize: "11.5px" }}>
          <PipelineConfigSections
            pipelineId={pipelineId}
            pipelineName={pipeline.name}
            attributes={(pipelines ?? []).find((p) => p.id === pipelineId)?.attributes}
          />
        </div>
      )}

      {!run && (
        <div className="px-3 py-3" style={{ fontSize: "11.5px" }}>
          <div
            className="flex items-center gap-2 rounded border border-dashed border-line-soft bg-bg-3 px-3 py-2.5 text-fg-4"
            style={{ fontSize: "11.5px" }}
          >
            <Info size={14} className="shrink-0" />
            <span>
              {hasAssistant
                ? "This is a template. Use the Assistant tab to author it in natural language; the Manager tab becomes available while a Run is in progress."
                : "No active run. The Manager tab becomes available while a Run is in progress."}
            </span>
          </div>
        </div>
      )}
    </>
  );
}

