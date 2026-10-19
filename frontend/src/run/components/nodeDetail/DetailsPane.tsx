import { useMemo, type ReactNode } from "react";
import { MessageSquare } from "lucide-react";
import type { NodeState } from "../../../shared/generated/NodeState";
import type { PortIO, FileInfo } from "../../api/nodes";
import type { PortType } from "../../../shared/generated/PortType";
import type { ChildCounts, RunChildEntry } from "../../model/orchestration";
// #723 — Orchestration tab + pastilles.
import { ChildCountPills, OrchestrationTab } from "../OrchestrationTab";
import IOSection from "./IOSection";
import LiveSection from "./LiveSection";
import type { LiveArtifactIO } from "../../../shared/generated/LiveArtifactIO";
import PromptSection from "./PromptSection";
import TranscriptTab from "./transcript/TranscriptTab";
import ExtensionDetailTab from "../../../shared/extensions/ExtensionDetailTab";
import { useDetailTabs } from "../../../shared/extensions/catalogue";

/**
 * The lower pane of the panel: the actions, then the tabs — I/O, Transcript,
 * Orchestration for an orchestrator node, the extensions' — then the selected
 * tab: Inputs / Outputs / Live artifacts / Initial Prompt under I/O.
 */
export default function DetailsPane({
  actions,
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
  inputs,
  outputs,
  live = [],
  promptText,
  onOpenFile,
  selectedIter,
  onSelectIter,
  iterLive,
}: {
  actions: ReactNode;
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
  return (
    <div
      className="flex h-full flex-col overflow-auto"
      data-testid="details-pane"
    >
      {/* Actions */}
      {actions}

      {/* I/O | Transcript (| Orchestration for an orchestrator node, #723).
          Below « Mark complete » (the escape hatch stays reachable from
          every tab), above the tab's content. */}
      <div className="flex shrink-0 overflow-x-auto border-b border-line" data-testid="detail-tabs">
        {coreTabs.map((tab) => {
          const active =
            extensionTab === null && (detailTab === tab || (tab === "io" && !isCoreTab(detailTab)));
          return (
            <button
              key={tab}
              type="button"
              data-testid={`detail-tab-${tab}`}
              data-active={active}
              onClick={() => onSelectTab(tab)}
              className={`flex flex-1 cursor-pointer items-center justify-center gap-1.5 py-1.5 transition-colors ${
                active ? "border-b-2 border-acc font-medium text-fg" : "text-fg-3 hover:text-fg-2"
              }`}
              style={{ fontSize: "11px" }}
            >
              {tab === "transcript" && <MessageSquare size={11} />}
              {tab === "io" ? "I/O" : tab === "transcript" ? "Transcript" : "Orchestration"}
              {tab === "orchestration" && <ChildCountPills counts={childCounts} size="xs" testId="tab-child-pills" />}
            </button>
          );
        })}
        {extensionTabs.map((t) => {
          const active = extensionTab?.key === t.key;
          return (
            <button
              key={t.key}
              type="button"
              data-testid={`detail-tab-ext-${t.key}`}
              data-active={active}
              onClick={() => onSelectTab(t.key)}
              className={`flex flex-1 cursor-pointer items-center justify-center gap-1.5 whitespace-nowrap px-2 py-1.5 transition-colors ${
                active ? "border-b-2 border-acc font-medium text-fg" : "text-fg-3 hover:text-fg-2"
              }`}
              style={{ fontSize: "11px" }}
            >
              {t.contribution.label}
            </button>
          );
        })}
      </div>
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
          iterations={node.iterations ?? []}
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
        />
      ) : (
        <>
      {/* Inputs section */}
      {inputs.length > 0 && (
        <IOSection
          title="Inputs"
          kind="input"
          ports={inputs}
          runId={runId}
          nodeId={node.node_id}
          onOpenFile={(portName, files, portType) =>
            onOpenFile("input", portName, files, portType)
          }
        />
      )}

      {/* Outputs section */}
      {outputs.length > 0 && (
        <IOSection
          title="Outputs"
          kind="output"
          ports={outputs}
          runId={runId}
          nodeId={node.node_id}
          showFrontmatter
          onOpenFile={(portName, files, portType) =>
            onOpenFile("output", portName, files, portType)
          }
        />
      )}

      {/* ADR-0089: Live artifacts, under the Outputs, apart from them. */}
      {live.length > 0 && (
        <LiveSection
          live={live}
          runId={runId}
          onOpen={(artifact) =>
            onOpenFile("live", artifact.name, artifact.files, artifact.port_type)
          }
        />
      )}

      {/* Initial Prompt */}
      <PromptSection
        promptText={promptText}
        status={node.status}
        isArchived={isArchived}
      />
        </>
      )}
    </div>
  );
}
