import { useMemo, type ReactNode } from "react";
import { Copy } from "lucide-react";
import { IconButton, StatusIcon, toast } from "../../../shared/ui/kit";
import { copyToClipboard } from "../../../shared/lib/clipboard";
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

/** The one-letter glyph of a harness, its full name in the tooltip. */
const HARNESS_LETTER: Record<string, string> = { claude: "C", opencode: "O", copilot: "G", codex: "X", pi: "P" };

export function HarnessGlyph({ harness }: { harness: string }) {
  const tip = `${harness} · frozen at spawn`;
  return (
    <span className="ri-hglyph" data-testid="node-frozen-harness" title={tip} aria-label={tip} role="img">
      {HARNESS_LETTER[harness] ?? harness.slice(0, 1).toUpperCase()}
    </span>
  );
}

/**
 * The node header, one 32 px line: the status icon, the name, the frozen
 * harness, then the iteration on screen, its duration (Total or Active at a
 * click), the cost, the extensions' `node/toolbar` actions (#30) and a copy
 * of the node id. `trailing` takes the node's controls when the tab bar does
 * not carry them.
 */
export default function NodeDetailHeader({
  node,
  nodeName,
  hasMultipleIters,
  selectedIter,
  onSelectIter,
  statusLabel,
  trailing,
}: {
  node: NodeState;
  /** Overrides the status label: an on-demand node not launched reads « On
   *  demand », not « Pending » (ADR-0086 — it waits for the user's ▶). */
  statusLabel?: string;
  nodeName?: string | null;
  hasMultipleIters: boolean;
  selectedIter: number;
  onSelectIter: (iter: number) => void;
  trailing?: ReactNode;
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
  const name = nodeName ?? node.node_id;
  const label = statusLabel ?? STATUS_LABELS[node.status] ?? node.status;
  return (
    <header className="ri-nh" data-testid="node-detail-header">
      <StatusIcon kind="node" status={node.status} title={label} />
      <span data-testid="node-status-badge" className="sr-only">
        {label}
      </span>
      <h2 className="ri-nh__name" title={`${name} · ${node.node_id}`}>
        {name}
      </h2>
      {/* #616/ADR-0046: the harness this node's session was FROZEN on — what
          actually ran, distinct from the run-level default. Absent for a node
          that never started (a pure skip) or a pre-#616 daemon. */}
      {node.harness && <HarnessGlyph harness={node.harness} />}
      <span className="ri-nh__fill" />
      <span className="ri-nh__meta">
        {hasMultipleIters ? (
          <IterSelector iterations={iterations} selectedIter={selectedIter} onSelect={onSelectIter} />
        ) : (
          <span title="Iteration">i{node.iter}</span>
        )}
      </span>
      {/* The duration of the SELECTED iteration; its start and end live in
          the tooltip. A click switches it between Total and Active. */}
      <span className="ri-nh__meta inline-flex items-center">
        <IterationDurationLabel key={selectedIter} timing={timing} mode={durationMode} onToggle={toggleDurationMode} />
        {node.cost && (
          <span data-testid="node-cost" className="ri-nh__cost" title={nodeCostTitle(node.cost)}>
            {" · "}
            {formatCostAmount(node.cost.usd, node.cost.partial, node.cost.form !== "reported")}
          </span>
        )}
      </span>
      <ExtensionActionButtons location="node/toolbar" extra={extra} className="flex items-center gap-px" buttonClassName="d-icon" iconSize={14} />
      <IconButton
        icon={Copy}
        size="sm"
        label="Copy node id"
        data-testid="copy-node-id"
        onClick={() => void copyToClipboard(node.node_id).then((ok) => ok && toast("Copied", "success"))}
      />
      {trailing}
    </header>
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
    <div className="border-b border-line-soft p-2">
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
