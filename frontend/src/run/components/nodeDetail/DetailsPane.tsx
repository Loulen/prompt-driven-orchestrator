import { useMemo, type ReactNode } from "react";
import type { NodeState } from "../../../shared/generated/NodeState";
import type { PortIO, FileInfo } from "../../api/nodes";
import type { PortType } from "../../../shared/generated/PortType";
import type { ChildCounts, RunChildEntry } from "../../model/orchestration";
// #723 — Orchestration tab + pastilles.
import { ChildCountPills, OrchestrationTab } from "../OrchestrationTab";
import IOSection from "./IOSection";
import PromptSection from "./PromptSection";
import ExtensionDetailTab from "../../../shared/extensions/ExtensionDetailTab";
import { useDetailTabs } from "../../../shared/extensions/catalogue";

/**
 * The lower pane of the panel: the actions, then (for an orchestrator node) the
 * I/O | Orchestration tabs, then Inputs / Outputs / Initial Prompt.
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
  promptText,
  onOpenFile,
}: {
  actions: ReactNode;
  node: NodeState;
  runId: string;
  isArchived?: boolean;
  isOrchestratorNode: boolean;
  nodeName?: string | null;
  /** `io`, `orchestration`, or the key of a `node/detail` tab of an extension. */
  detailTab: string;
  onSelectTab: (tab: string) => void;
  childCounts: ChildCounts;
  nodeChildren: RunChildEntry[];
  onOpenChildRun?: (childRunId: string) => void;
  inputs: PortIO[];
  outputs: PortIO[];
  promptText: string | null;
  onOpenFile: (
    portKind: "input" | "output",
    portName: string,
    files: FileInfo[],
    portType: PortType,
  ) => void;
}) {
  // ADR-0081 (#31): the `node/detail` tabs extensions add, after I/O and
  // Orchestration. The tab bar shows once there is a second tab.
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
  const coreTabs = isOrchestratorNode ? (["io", "orchestration"] as const) : (["io"] as const);
  const showTabs = isOrchestratorNode || extensionTabs.length > 0;
  return (
    <div
      className="flex h-full flex-col overflow-auto"
      data-testid="details-pane"
    >
      {/* Actions */}
      {actions}

      {/* #723 — I/O | Orchestration tabs, only for an orchestrator node.
          Below « Mark complete » (the escape hatch stays reachable from
          both tabs), above the I/O content. */}
      {showTabs && (
        <div className="flex shrink-0 overflow-x-auto border-b border-line" data-testid="detail-tabs">
          {coreTabs.map((tab) => {
            const active = extensionTab === null && (detailTab === tab || (tab === "io" && detailTab !== "orchestration"));
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
                {tab === "io" ? "I/O" : "Orchestration"}
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
      )}
      {extensionTab ? (
        <ExtensionDetailTab
          key={`${extensionTab.key}:${node.node_id}`}
          tab={extensionTab}
          context={extensionContext}
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
