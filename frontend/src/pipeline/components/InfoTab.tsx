/**
 * The Info tab: a pipeline's metadata (editable on a template, read-only on a
 * Run), a Run's status, source and stats.
 */

import { useMemo } from "react";
import { Clock, Coins, FileCode, GitCompare, Layers } from "lucide-react";
import type { RunState } from "../../shared/generated/RunState";
import { isLiveRun } from "../../run/model/runStatus";
import { formatDuration, useRunDuration } from "../../run/model/runDuration";
import { formatEstCost } from "../../run/model/costLabel";
import { waitingForStartedNodesLabel } from "../../run/model/nodePlay";
import { Banner, Chip, KeyValue, Section, StatusIcon } from "../../shared/ui/kit";
import type { EditorPipelineDef } from "../model/pipeline";

import { SourceBlock } from "./SourceBlock";
import PipelineMetaSections from "./PipelineMetaSections";
import { PipelineConfigSections } from "./ExtensionInspectorSections";
import { inspectorSection } from "./inspectorSection";
import { useEditStore } from "../store/editStore";
import { InfoVersionLine, NewerVersionPill } from "./InfoVersion";
import { shortSavedAgo } from "../model/versions";
import ExtensionSections from "../../shared/extensions/ExtensionSections";
import type { RunInfoContext } from "../../shared/extensions/contract";
import "./inspect.css";

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
  return <ExtensionSections<RunInfoContext> location="run/info" extra={extra} wrap={inspectorSection} />;
}

/** One cell of the stats strip: an icon, the value in mono, its name for the tooltip and the screen reader. */
function Stat({
  icon: Icon,
  label,
  title,
  live,
  testid,
  children,
}: {
  icon: typeof Clock;
  label: string;
  title?: string;
  live?: boolean;
  testid: string;
  children: React.ReactNode;
}) {
  return (
    <span className="pi-stats__cell" title={title ?? label} data-live={live || undefined} data-testid={testid}>
      <Icon size={13} strokeWidth={1.5} aria-hidden />
      <span className="d-sr">{label}</span>
      {children}
    </span>
  );
}

function RunStats({ run, onOpenDiff }: { run: RunState; onOpenDiff: () => void }) {
  const durationMs = useRunDuration(run.started_at, run.completed_at, run.status);
  const durationLabel = formatDuration(durationMs);
  const ticking = run.completed_at == null && isLiveRun(run.status);
  // Shared honesty helper (#272/#377): same vocabulary as the aggregated Stats
  // charts. #553: « — » and a reason when a node ran on a harness with no cost
  // source, never a misleading $0; #615: a mixed Run's total by harness.
  const cost = run.cost
    ? formatEstCost(
        run.cost.usd,
        run.cost.partial,
        run.cost.unpriced_models,
        run.cost.uncosted_harnesses ?? [],
        run.cost.by_harness ?? [],
      )
    : null;
  // The per-harness breakdown only when it says more than the total does: a
  // genuinely mixed Run (≥ 2 slices), or any slice when the total is withheld.
  const slices = cost?.ventilation ?? [];
  const ventilated = cost != null && slices.length > (cost.text === "—" ? 0 : 1);
  const files = run.loc ? `${run.loc.files_changed.toLocaleString()} ${run.loc.files_changed === 1 ? "file" : "files"}` : "";
  return (
    <div className="pi-stats" data-testid="run-stats">
      <Stat icon={Clock} label="Duration" live={ticking} testid="stat-duration">
        {durationLabel ?? "—"}
        {ticking && <span className="d-sr" data-testid="stat-duration-live">live</span>}
      </Stat>
      <Stat icon={Layers} label="Node sessions started" testid="stat-sessions">
        {(run.sessions_spawned ?? 0).toLocaleString()}
      </Stat>
      <Stat icon={GitCompare} label="Changes" title={run.loc ? `Changes · ${files}` : "Changes"} testid="stat-loc">
        {run.loc ? (
          // #748: the LOC stat opens the Diff tab — the count IS what the tab
          // shows (same endpoint bounds), so the number links to it.
          <button type="button" className="pi-stats__link" onClick={onOpenDiff} data-testid="stat-loc-open-diff">
            <span className="ei-add">+{run.loc.insertions.toLocaleString()}</span>
            <span className="ei-del">−{run.loc.deletions.toLocaleString()}</span>
            <span className="d-sr">· {files}</span>
          </button>
        ) : (
          "—"
        )}
      </Stat>
      <Stat icon={Coins} label="Est. cost" title={cost?.title ?? "Est. cost"} testid="stat-cost">
        {cost ? (
          <span title={cost.title}>
            {cost.text}
            {cost.dagger && <span className="ei-warn">†</span>}
            {ventilated && (
              <span className="pi-sub"> · {slices.map((v) => `${v.text} via ${v.harness}`).join(" · ")}</span>
            )}
          </span>
        ) : (
          "—"
        )}
      </Stat>
    </div>
  );
}

/** `4m ago`, `3h ago`, `2d ago`: when the Run started. */
function startedAgo(iso: string): string {
  const ago = shortSavedAgo(iso);
  return ago === "now" || ago === "" ? ago || "—" : `${ago} ago`;
}

export function InfoTab({
  run,
  pipeline,
  pipelineName,
  pipelineId,
  onOpenDiff,
}: {
  run: RunState | null;
  pipeline: EditorPipelineDef | null;
  pipelineName: string;
  /** The template's id (its edit tab), for the `pipeline/config` sections; null on a Run. */
  pipelineId: string | null;
  /** Kept for the hosts that still pass it: the Assistant is a tab of its own. */
  hasAssistant?: boolean;
  /** #748: the Changes stat is the link to what it counts — the Diff tab. */
  onOpenDiff: () => void;
}) {
  const pipelines = useEditStore((s) => s.pipelines);
  // ADR-0088: a version shown in its own tab is read only, like a Run.
  const viewingVersion = useEditStore(
    (s) => s.openTabs.find((t) => t.id === s.activeTabId)?.version != null,
  );
  const waiting = run?.waiting_for_started_nodes ?? 0;

  return (
    <div data-testid="info-tab">
      <div className="pi-head">
        {run ? (
          <StatusIcon kind="run" status={run.status} />
        ) : (
          <FileCode size={14} strokeWidth={1.5} aria-hidden className="ei-ic" />
        )}
        <span className="pi-head__name" title={pipelineName} data-testid="info-panel-name">
          {pipelineName}
        </span>
        {run ? (
          <span className="d-mono ei-dim" title={run.run_id}>
            {run.run_id.slice(-7)}
          </span>
        ) : (
          <Chip>template</Chip>
        )}
        <NewerVersionPill run={run} />
        {/* #315: a Run's settings are read-only; canvas edits apply to it only. */}
        {run && (
          <span data-testid="run-info-note">
            <Chip
              title={
                run.status === "archived"
                  ? "Archived run · read-only · outputs preserved"
                  : "Canvas edits apply to this run only · run settings are read-only"
              }
            >
              {run.status === "archived" ? "archived" : "read only"}
            </Chip>
          </span>
        )}
      </div>
      <InfoVersionLine run={run} pipelineId={pipelineId} />

      {/* #752: the failure / awaiting reason (#503, #598) first: clicking a red
          dot lands on this tab. */}
      {run?.failure_reason && (
        <div data-testid="run-failure-reason">
          <Banner
            tone={run.status === "halted" ? "blocked" : run.status === "skipped" ? "skipped" : "failed"}
            title={run.status === "halted" ? "Halted" : run.status === "skipped" ? "Skipped" : "Failed"}
            text={run.failure_reason}
          />
        </div>
      )}
      {run?.awaiting_reason && (
        <div data-testid="run-awaiting-reason">
          {/* #588: only an incident carries a machine slug; a declared wait is
              the agent's question (or an awaiting child), not an interruption. */}
          <Banner
            tone="awaiting"
            title={run.awaiting_reason_code ? "Interrupted · awaiting you" : "Awaiting you"}
            text={run.awaiting_reason}
          />
        </div>
      )}
      {waiting > 0 && (
        <div data-testid="run-waiting-started-nodes-reason">
          <Banner tone="awaiting" title="Reached End" text={waitingForStartedNodesLabel(waiting)} />
        </div>
      )}
      {/* #804 / ADR-0070 §1: a Trigger fire that could not refresh the remotes
          before cutting. Not a failure — the Run ran, from the last state known
          locally. git's own words in full on hover. */}
      {run?.source_fetch_error && (
        <div data-testid="run-source-fetch-error">
          <Banner
            tone="skipped"
            title="Started on local state · fetch failed"
            text={run.source_branch ?? "source ref"}
          />
          <div className="ei-idline" title={run.source_fetch_error.message} data-testid="run-source-fetch-stderr">
            {run.source_fetch_error.message}
          </div>
        </div>
      )}
      {/* Sandbox preparation (#410): the image is pulled or built at first use,
          only while `sandbox_prep === "pending"`. */}
      {run?.sandbox_prep === "pending" && (
        <div data-testid="sandbox-prep-banner">
          <Banner tone="running" title="Preparing the sandbox" text="pulling or building the image" />
        </div>
      )}

      {run && (
        <Section title="Run">
          <KeyValue
            rows={[
              {
                k: "Status",
                v: (
                  <span className="ei-line" data-testid="run-status">
                    <StatusIcon kind="run" status={run.status} size="sm" />
                    {run.status.replace(/_/g, " ")}
                  </span>
                ),
                tip: run.status,
              },
              ...(run.harness
                ? [{ k: "Harness", v: <span data-testid="run-harness">{run.harness}</span>, mono: true, tip: run.harness }]
                : []),
              ...(run.target_repo ? [{ k: "Repo", v: run.target_repo, mono: true }] : []),
              // Sandbox (#410): any sandboxed Run (full/minimal); an `off`/host Run has no row.
              ...(run.sandbox && run.sandbox !== "off"
                ? [
                    {
                      k: "Sandbox",
                      v: <span data-testid="sandbox-badge">{run.sandbox}</span>,
                      mono: true,
                      tip: `Docker sandbox (${run.sandbox})`,
                    },
                  ]
                : []),
              ...(run.started_at
                ? [
                    {
                      k: "Started",
                      v: <span data-testid="run-started">{startedAgo(run.started_at)}</span>,
                      tip: Number.isNaN(Date.parse(run.started_at)) ? run.started_at : new Date(run.started_at).toLocaleString(),
                    },
                  ]
                : []),
              ...(run.triggered_by ? [{ k: "Trigger", v: run.triggered_by, mono: true }] : []),
            ]}
          />
          <SourceBlock run={run} />
        </Section>
      )}

      {run && (
        <Section title="Stats" flush>
          <RunStats run={run} onOpenDiff={onOpenDiff} />
        </Section>
      )}

      {run && <RunInfoSections run={run} />}

      {/* Notion #6 / #948: Pipeline info is the only surface of the pipeline's
          metadata — editable on a template, read-only on a Run (active or
          archived) and on a version. */}
      {pipeline && <PipelineMetaSections pipeline={pipeline} readOnly={run != null || viewingVersion} />}

      {/* On a Run and a version the node and edge counts are rows of the
          Pipeline section; a template keeps them in a section of their own. */}
      {pipeline && run == null && !viewingVersion && (
        <div data-testid="info-stats">
          <Section title="Graph">
            <KeyValue
              rows={[
                { k: "Nodes", v: <span data-testid="pipeline-graph-nodes">{pipeline.nodes.length}</span>, mono: true },
                { k: "Edges", v: <span data-testid="pipeline-graph-edges">{pipeline.edges.length}</span>, mono: true },
              ]}
            />
          </Section>
        </div>
      )}

      {/* ADR-0081 (#31): the `pipeline/config` sections of the extensions, after
          a template's metadata — they followed the standalone inspector here (#948). */}
      {!run && pipeline && pipelineId && (
        <PipelineConfigSections
          pipelineId={pipelineId}
          pipelineName={pipeline.name}
          attributes={(pipelines ?? []).find((p) => p.id === pipelineId)?.attributes}
        />
      )}
    </div>
  );
}
