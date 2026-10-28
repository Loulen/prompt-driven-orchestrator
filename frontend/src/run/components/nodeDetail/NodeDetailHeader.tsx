import { useMemo } from "react";
import type { NodeState } from "../../../shared/generated/NodeState";
import { ExtensionActionButtons } from "../../../shared/extensions/ExtensionActions";
import type { NodeEntity } from "../../../shared/extensions/contract";
import type { ScopedProvisioningRules } from "../../../shared/generated/ScopedProvisioningRules";
import { formatCostAmount, nodeCostTitle } from "../../model/costLabel";
import ProvisioningRulesEditor from "../../../workspace/components/ProvisioningRulesEditor";
import {
  EMPTY_PROVISIONING_RULES,
  hasProvisioningRules,
} from "../../../workspace/model/provisioning";
import IterSelector from "./IterSelector";
import { IterationDurationLabel } from "./IterationDuration";
import { formatTime } from "../../model/nodeDetailFormat";
import { nodeIterationTimings } from "../../model/iterationDuration";
import type { IterationTiming } from "../../model/iterationDuration";
import { useNodeDurationMode } from "../../store/nodeDurationModeStore";
import { STATUS_LABELS } from "./labels";

/** The panel header: name, status, frozen harness, the extensions' `node/toolbar`
 *  actions (#30), iter selector, duration of the iteration on show, cost. */
export default function NodeDetailHeader({
  node,
  nodeName,
  hasMultipleIters,
  selectedIter,
  onSelectIter,
  statusLabel,
}: {
  node: NodeState;
  /** Overrides the status label: an on-demand node not launched reads « On
   *  demand », not « Pending » (ADR-0086 — it waits for the user's ▶). */
  statusLabel?: string;
  nodeName?: string | null;
  hasMultipleIters: boolean;
  selectedIter: number;
  onSelectIter: (iter: number) => void;
}) {
  const extra = useMemo(() => {
    const entity: NodeEntity = {
      kind: "node",
      id: node.node_id,
      name: nodeName ?? null,
      type: null,
      status: node.status,
      attributes: {},
    };
    return { entity };
  }, [node.node_id, node.status, nodeName]);
  const iterations = useMemo(() => nodeIterationTimings(node), [node]);
  // The duration of the iteration on show, not always the latest one's.
  const current = selectedIter === node.iter;
  const timing: IterationTiming = iterations.find((it) => it.iter === selectedIter) ?? {
    status: node.status,
    started_at: current ? node.started_at : null,
    completed_at: current ? node.completed_at : null,
  };
  // The Mode de durée on screen — one global, persisted preference.
  const durationMode = useNodeDurationMode((st) => st.mode);
  const toggleDurationMode = useNodeDurationMode((st) => st.toggle);
  return (
    <div className="border-b border-line px-3 py-2">
      <div className="flex items-center gap-2">
        <span className="font-medium text-text" style={{ fontSize: "12.5px" }}>
          {nodeName ?? node.node_id}
        </span>
        <span
          data-testid="node-status-badge"
          className="rounded border border-line-strong bg-surface-2 px-1.5 py-0.5 text-text-3"
          style={{ fontSize: "10px", fontWeight: 500 }}
        >
          {statusLabel ?? STATUS_LABELS[node.status] ?? node.status}
        </span>
        {/* #616/ADR-0046: the harness this node's session was FROZEN on, next to
            the id — so the Run view shows what actually ran (distinct from the
            run-level default in the pipeline info panel). Absent for a node that
            never started (a pure skip) or a pre-#616 daemon. */}
        {node.harness && (
          <span
            data-testid="node-frozen-harness"
            title="Harness frozen at spawn"
            className="rounded border border-primary/40 bg-primary/5 px-1.5 py-0.5 font-mono text-primary"
            style={{ fontSize: "10px" }}
          >
            {node.harness}
          </span>
        )}
        <ExtensionActionButtons
          location="node/toolbar"
          extra={extra}
          className="ml-auto flex items-center gap-0.5"
        />
      </div>
      <div className="mt-0.5 font-mono text-text-3" style={{ fontSize: "9px" }}>
        {node.node_id}
      </div>
      <div
        className="mt-0.5 flex flex-wrap items-center gap-x-1 whitespace-nowrap font-mono text-text-3"
        style={{ fontSize: "10px" }}
      >
        {hasMultipleIters ? (
          <IterSelector
            iterations={iterations}
            selectedIter={selectedIter}
            onSelect={onSelectIter}
          />
        ) : (
          <span>iter {node.iter}</span>
        )}
        {/* The duration of the SELECTED iteration; its start and end live in
            the tooltip. A click switches it between Total and Active. */}
        <IterationDurationLabel
          key={selectedIter}
          timing={timing}
          mode={durationMode}
          onToggle={toggleDurationMode}
        />
        {node.cost && (
          <span data-testid="node-cost" title={nodeCostTitle(node.cost)}>
            {" · "}
            {formatCostAmount(
              node.cost.usd,
              node.cost.partial,
              node.cost.form !== "reported",
            )}
          </span>
        )}
      </div>
    </div>
  );
}

/** The read-only provisioning rules of an isolated node (own + inherited). */
export function NodeProvisioning({
  node,
  provisioningRepository,
  inheritedProvisioning,
  provisioningGitRef,
}: {
  node: NodeState;
  provisioningRepository: string;
  inheritedProvisioning?: ScopedProvisioningRules[];
  provisioningGitRef: string;
}) {
  if (
    !(
      node.isolated_worktree &&
      (hasProvisioningRules(node.provisioning ?? EMPTY_PROVISIONING_RULES) ||
        (inheritedProvisioning?.some(({ rules }) => hasProvisioningRules(rules)) ??
          false))
    )
  ) {
    return null;
  }
  return (
    <div className="border-b border-line p-2">
      <ProvisioningRulesEditor
        level="isolated_node"
        repository={provisioningRepository}
        rules={node.provisioning ?? EMPTY_PROVISIONING_RULES}
        onChange={() => {}}
        readOnly
        frozenAt={
          node.provisioning_frozen_at
            ? formatTime(node.provisioning_frozen_at)
            : undefined
        }
        frozenPlan={node.provisioning_plan}
        inherited={inheritedProvisioning}
        gitRef={provisioningGitRef}
      />
    </div>
  );
}
