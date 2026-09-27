import type { ReactNode } from "react";
import type { NodeState } from "../../../shared/generated/NodeState";
import type { PortIO, FileInfo } from "../../api/nodes";
import type { PortType } from "../../../shared/generated/PortType";
import type { ChildCounts, RunChildEntry } from "../../model/orchestration";
// #723 — Orchestration tab + pastilles.
import { ChildCountPills, OrchestrationTab } from "../OrchestrationTab";
import IOSection from "./IOSection";
import PromptSection from "./PromptSection";

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
  detailTab: "io" | "orchestration";
  onSelectTab: (tab: "io" | "orchestration") => void;
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
      {isOrchestratorNode && (
        <div className="flex shrink-0 border-b border-line" data-testid="detail-tabs">
          {(["io", "orchestration"] as const).map((tab) => {
            const active = detailTab === tab;
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
        </div>
      )}
      {isOrchestratorNode && detailTab === "orchestration" ? (
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
