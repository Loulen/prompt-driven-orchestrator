import { useMemo, type ReactNode } from "react";
import { ArrowLeftRight, MessageSquare, Network } from "lucide-react";
import { KeyValue, Section, StatusIcon, type KeyValueRow } from "../../../shared/ui/kit";
import { formatCostAmount, nodeCostTitle } from "../../model/costLabel";
import { formatTime } from "../../model/nodeDetailFormat";
import { STATUS_LABELS } from "./labels";
import type { NodeState } from "../../../shared/generated/NodeState";
import type { PortIO, FileInfo } from "../../api/nodes";
import type { PortType } from "../../../shared/generated/PortType";
import type { ChildCounts, RunChildEntry } from "../../model/orchestration";
import type { TreeCost } from "../../../shared/generated/TreeCost";
// #723 — Orchestration tab + pastilles.
import { ChildCountPills, OrchestrationTab } from "../OrchestrationTab";
import IOSection from "./IOSection";
import LiveSection from "./LiveSection";
import type { LiveArtifactIO } from "../../../shared/generated/LiveArtifactIO";
import PromptSection from "./PromptSection";
import TranscriptTab from "./transcript/TranscriptTab";
import { nodeIterationTimings } from "../../model/iterationDuration";
import ExtensionDetailTab from "../../../shared/extensions/ExtensionDetailTab";
import { useDetailTabs } from "../../../shared/extensions/catalogue";

/**
 * The lower pane of the panel: the tabs — I/O, Transcript, Orchestration for
 * an orchestrator node, the extensions', Approve ▾ pinned at the right of
 * them — then the selected tab: Outputs,
 * Inputs, Live artifacts, Properties and Prompt under I/O.
 */
export default function DetailsPane({
  node,
  runId,
  isArchived,
  isOrchestratorNode,
  nodeName = null,
  detailTab,
  onSelectTab,
  childCounts,
  nodeChildren,
  onOpenChildRun,
  runTreeCost = null,
  inputs,
  outputs,
  live = [],
  promptText,
  onOpenFile,
  selectedIter,
  onSelectIter,
  iterLive,
  barEnd,
}: {
  node: NodeState;
  runId: string;
  isArchived?: boolean;
  isOrchestratorNode: boolean;
  nodeName?: string | null;
  /** `io`, `transcript`, `orchestration`, or the key of a `node/detail` tab of
   *  an extension. */
  detailTab: string;
  onSelectTab: (tab: string) => void;
  childCounts: ChildCounts;
  nodeChildren: RunChildEntry[];
  onOpenChildRun?: (childRunId: string) => void;
  /** The Run's Coût de l'arbre, at the foot of the Orchestration tab. */
  runTreeCost?: TreeCost | null;
  inputs: PortIO[];
  outputs: PortIO[];
  /** ADR-0089: the iteration's live artifacts. */
  live?: LiveArtifactIO[];
  promptText: string | null;
  onOpenFile: (
    portKind: "input" | "output" | "live",
    portName: string,
    files: FileInfo[],
    portType: PortType,
  ) => void;
  /** The iteration on screen, which the Transcript tab follows and can change. */
  selectedIter: number;
  onSelectIter: (iter: number) => void;
  /** The iteration on screen is running: its transcript is followed live. */
  iterLive: boolean;
  /** Pinned at the right of the tab bar while the tabs scroll: Approve ▾. */
  barEnd?: ReactNode;
}) {
  // ADR-0081 (#31): the `node/detail` tabs extensions add, after the core's.
  // The tab bar always shows: I/O and Transcript are there for every node.
  const entity = useMemo(
    () => ({
      kind: "node",
      id: node.node_id,
      attributes: {},
      ...(nodeName ? { name: nodeName } : {}),
    }),
    [node.node_id, nodeName],
  );
  const { tabs: extensionTabs, context: extensionContext } = useDetailTabs(entity, "node/detail");
  const extensionTab = extensionTabs.find((t) => t.key === detailTab) ?? null;
  const coreTabs = isOrchestratorNode
    ? (["io", "transcript", "orchestration"] as const)
    : (["io", "transcript"] as const);
  const isCoreTab = (tab: string) => (coreTabs as readonly string[]).includes(tab);
  const tabButton = (key: string, active: boolean, label: ReactNode, icon: ReactNode, testId: string, title: string) => (
    <button
      key={key}
      type="button"
      role="tab"
      aria-selected={active}
      data-testid={testId}
      data-active={active}
      title={title}
      onClick={() => onSelectTab(key)}
      className="ri-tab"
    >
      {icon}
      {label}
    </button>
  );
  return (
    <div className="ri-dtabs" data-testid="details-pane">
      {/* I/O | Transcript (| Orchestration for an orchestrator node, #723) |
          the extensions' tabs, above the tab's content. */}
      <div className="ri-dtabs__bar" data-testid="detail-tabs-bar">
        <div className="ri-dtabs__tabs" role="tablist" aria-label="Node details" data-testid="detail-tabs">
          {coreTabs.map((tab) => {
            const active =
              extensionTab === null && (detailTab === tab || (tab === "io" && !isCoreTab(detailTab)));
            const [Icon, label, title] =
              tab === "io"
                ? [ArrowLeftRight, "I/O", "Inputs, outputs, prompt"]
                : tab === "transcript"
                  ? [MessageSquare, "Transcript", "Transcript"]
                  : [Network, "Orchestration", "Child runs"];
            return tabButton(
              tab,
              active,
              <>
                <span className="ri-tab__label">{label}</span>
                {tab === "orchestration" && <ChildCountPills counts={childCounts} size="pill" testId="tab-child-pills" />}
              </>,
              <Icon size={13} strokeWidth={1.5} aria-hidden />,
              `detail-tab-${tab}`,
              title,
            );
          })}
          {extensionTabs.map((t) =>
            tabButton(t.key, extensionTab?.key === t.key, t.contribution.label, null, `detail-tab-ext-${t.key}`, t.contribution.label),
          )}
        </div>
        {barEnd && <div className="ri-dtabs__end">{barEnd}</div>}
      </div>
      <div className="ri-dtabs__body" role="tabpanel">
      {extensionTab ? (
        <ExtensionDetailTab
          key={`${extensionTab.key}:${node.node_id}`}
          tab={extensionTab}
          context={extensionContext}
        />
      ) : detailTab === "transcript" ? (
        <TranscriptTab
          key={`${node.node_id}:${selectedIter}`}
          runId={runId}
          nodeId={node.node_id}
          iter={selectedIter}
          iterations={nodeIterationTimings(node)}
          onSelectIter={onSelectIter}
          live={iterLive}
        />
      ) : isOrchestratorNode && detailTab === "orchestration" ? (
        <OrchestrationTab
          childRuns={nodeChildren}
          counts={childCounts}
          nodeLive={node.status === "running" || node.status === "awaiting_user"}
          nodeAwaiting={node.status === "awaiting_user"}
          onOpenChild={(id) => onOpenChildRun?.(id)}
          runTreeCost={runTreeCost}
        />
      ) : (
        <>
          {outputs.length > 0 && (
            <IOSection
              title="Outputs"
              kind="output"
              ports={outputs}
              runId={runId}
              nodeId={node.node_id}
              showFrontmatter
              onOpenFile={(portName, files, portType) => onOpenFile("output", portName, files, portType)}
            />
          )}
          {/* ADR-0089: Live artifacts, under the Outputs, apart from them. */}
          {live.length > 0 && (
            <LiveSection
              live={live}
              runId={runId}
              onOpen={(artifact) => onOpenFile("live", artifact.name, artifact.files, artifact.port_type)}
            />
          )}
          {inputs.length > 0 && (
            <IOSection
              title="Inputs"
              kind="input"
              ports={inputs}
              runId={runId}
              nodeId={node.node_id}
              onOpenFile={(portName, files, portType) => onOpenFile("input", portName, files, portType)}
            />
          )}
          <NodeProperties node={node} runId={runId} selectedIter={selectedIter} />
          <PromptSection promptText={promptText} status={node.status} isArchived={isArchived} />
        </>
      )}
      </div>
    </div>
  );
}

/** What the node ran on: its status, agent, worktree, session, start and cost. */
function NodeProperties({ node, runId, selectedIter }: { node: NodeState; runId: string; selectedIter: number }) {
  const iteration = node.iterations?.find((i) => i.iter === selectedIter);
  const status = iteration?.status ?? node.status;
  const startedAt = iteration?.started_at ?? (selectedIter === node.iter ? node.started_at : null);
  const rows: KeyValueRow[] = [
    {
      k: "Status",
      v: (
        <span className="inline-flex items-center gap-1">
          <StatusIcon kind="node" status={status} size="sm" />
          {STATUS_LABELS[status]}
        </span>
      ),
      tip: STATUS_LABELS[status],
    },
  ];
  if (node.harness) rows.push({ k: "Agent", v: node.harness, mono: true });
  if (node.isolated_worktree != null) rows.push({ k: "Worktree", v: node.isolated_worktree ? "isolated" : "run", mono: true });
  if (status !== "pending") rows.push({ k: "Session", v: `pdo-${runId}-${node.node_id}-iter-${selectedIter}`, mono: true });
  if (startedAt) rows.push({ k: "Started", v: formatTime(startedAt) });
  if (node.cost && node.cost.usd != null) {
    rows.push({
      k: "Cost",
      v: `${formatCostAmount(node.cost.usd, node.cost.partial, node.cost.form !== "reported")} · ${node.cost.executions}×`,
      mono: true,
      tip: nodeCostTitle(node.cost),
    });
  }
  return (
    <div data-testid="node-properties">
      <Section title="Properties">
        <KeyValue rows={rows} />
      </Section>
    </div>
  );
}
