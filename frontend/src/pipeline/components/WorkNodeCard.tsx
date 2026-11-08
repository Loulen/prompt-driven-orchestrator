/**
 * The card of a work node, as the canvas draws it and as the preview of
 * Settings › Appearance shows it: the frame, the header (the type icon and its
 * agent marker, the name, the flags, the status), the meta grid, an
 * orchestrator's « N runs ». On one line with « Compact nodes »: the icon, the
 * name, the status, the runs.
 *
 * `marks` says how each Élément masquable of a Run's card renders
 * (`model/runCardElements.ts`), `null` when it is hidden; the editor passes
 * none and shows everything. The type icon, the name, the status, the runs and
 * a broken agent profile (an alert) are fixed.
 */
import type { CSSProperties, ReactNode } from "react";
import { Bookmark, Play, SlidersHorizontal, TriangleAlert } from "lucide-react";
import type { ElementMarks } from "../../hooks/useElementMarks";
import { RUN_CARD } from "../model/runCardElements";
import type { EditNodeData } from "./EditNode";
import { NodeCard } from "./NodeCard";
import { NodeCardHead, OrchestratorRuns } from "./NodeCardParts";
import { NodeMeta } from "./NodeMeta";
import { IsolationMarker, NodeTypeIcon } from "./NodeTypeIcon";

/** The marks of an element in a card that hides nothing. */
export const SHOW_ALL = (): ElementMarks => ({});

export interface WorkNodeCardProps {
  data: EditNodeData;
  marks?: (id: string) => ElementMarks | null;
  selected?: boolean;
  /** An on-demand node the Run never launched. */
  idle?: boolean;
  rimHover?: boolean;
  wireTarget?: boolean;
  style?: CSSProperties;
  /** In the card, before its content: a presence label, the handles. */
  lead?: ReactNode;
  /** In the card, after its content: the rim, a search badge. */
  children?: ReactNode;
}

export function WorkNodeCard({
  data,
  marks = SHOW_ALL,
  selected,
  idle,
  rimHover,
  wireTarget,
  style,
  lead,
  children,
}: WorkNodeCardProps) {
  const inRun = data.inRun === true;
  const status = inRun ? data.status : null;
  const orchestrator = data.orchestrator === true;
  const runs = data.childCounts && <OrchestratorRuns counts={data.childCounts} onOpen={data.onOpenOrchestration} />;
  const icon = (
    <NodeTypeIcon type={data.nodeType} interactive={data.interactive} orchestrator={orchestrator} size={12} className="cv-n__type" />
  );
  if (data.compact) {
    // The harness icon, the name, the status: the detailed header's order. The
    // iteration, the worktree and profile markers stay in the inspector.
    return (
      <NodeCard compact status={status} selected={selected} idle={idle} orchestrator={orchestrator} style={style}>
        {lead}
        <NodeCardHead type={icon} name={data.label} status={status} />
        {runs}
        {children}
      </NodeCard>
    );
  }
  const profile = data.agentMode === "profile" || data.agentMode === "custom" ? marks(RUN_CARD.profile) : null;
  const isolation = marks(RUN_CARD.isolation);
  const loop = data.loopBadge ? marks(RUN_CARD.loop) : null;
  // The type icon leads, in the Theme's accent; the status closes the header.
  const typeIcon = (
    <>
      {icon}
      {profile && data.agentMode === "profile" && (
        <Bookmark data-testid="agent-mode-profile" size={8} className="cv-n__mode" aria-label="Agent profile" {...profile} />
      )}
      {profile && data.agentMode === "custom" && (
        <SlidersHorizontal data-testid="agent-mode-custom" size={8} className="cv-n__mode" aria-label="Custom agent" {...profile} />
      )}
      {data.agentMode === "broken" && (
        <TriangleAlert data-testid="agent-mode-broken" size={8} className="cv-n__mode" data-broken aria-label="Missing agent profile" />
      )}
    </>
  );
  const flags = (
    <>
      {isolation && <IsolationMarker isolated={data.isolated === true} marks={isolation} />}
      {data.onDemand && !inRun && (
        <span className="cv-n__flag" title="On demand" data-testid="on-demand-marker">
          <Play size={10} strokeWidth={1.5} aria-label="On demand" />
        </span>
      )}
      {data.loopBadge && loop && (
        <span
          data-testid={data.loopBadge.kind === "collection" ? "collection-badge" : "loop-badge"}
          className="cv-n__loop"
          data-budget={data.loopBadge.budgetReached || undefined}
          title={[
            data.loopBadge.kind === "collection" ? "Collection region" : "Loop region",
            data.loopBadge.costTitle,
          ]
            .filter(Boolean)
            .join("\n")}
          {...loop}
        >
          {data.loopBadge.text}
          {data.loopBadge.costText != null && (
            <>
              {" · "}
              <span data-testid="loop-badge-cost" className="cv-n__loop-cost" data-warn={data.loopBadge.costWarn || undefined}>
                {data.loopBadge.costText}
              </span>
            </>
          )}
        </span>
      )}
    </>
  );
  return (
    <NodeCard
      status={status}
      selected={selected}
      idle={idle}
      rimHover={rimHover}
      wireTarget={wireTarget}
      orchestrator={orchestrator}
      style={style}
    >
      {lead}
      <NodeCardHead type={typeIcon} name={data.label} flags={flags} status={status} />
      <NodeMeta data={data} run={data.run} marks={marks} />
      {data.loopBadge?.budgetReached && (
        <div className="cv-n__budget" data-testid="loop-badge-budget">
          budget reached · awaiting you
        </div>
      )}
      {/* #723 — a parent node's child runs (ADR-0064, toggle or not): « N runs »
          and the Orchestration tab's pastilles; omitted entirely without children. */}
      {runs}
      {children}
    </NodeCard>
  );
}
