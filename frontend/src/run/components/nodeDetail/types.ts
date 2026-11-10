import type { FileInfo } from "../../api/nodes";
import type { PortType } from "../../../shared/generated/PortType";
import type { NodeState } from "../../../shared/generated/NodeState";
import type { RunStatus } from "../../../shared/generated/RunStatus";
import type { ScopedProvisioningRules } from "../../../shared/generated/ScopedProvisioningRules";
import type { TreeCost } from "../../../shared/generated/TreeCost";

// The terminal inset has three mutually exclusive display modes (#346):
//   - "split"     : terminal ~45% / detail pane ~55% (default for a live node)
//   - "expanded"  : the « Terminal agrandi » (#44): the same terminal lifted
//                   over the whole app, 16px from the window edges, on a dimmed
//                   backdrop (user gesture, never persisted)
//   - "minimized" : terminal collapsed to a thin bar, Outputs take the full
//                   height (default when opening a node whose session ended)
// An enum (not two orthogonal booleans) makes the illegal
// `{minimized + expanded}` state unrepresentable.
export type TerminalView = "minimized" | "split" | "expanded";

export interface ModalState {
  portName: string;
  files: FileInfo[];
  portKind: "input" | "output" | "live";
  portType: PortType;
}

/**
 * The gestures on the node on screen the caller may not make (CORE-42), as
 * the question of authorization answered them: each one refused stays on
 * screen, greyed, its tooltip saying this sentence (CORE-205). All absent
 * before the answer, and without an extension that restricts.
 */
export interface NodeGestureRefusals {
  stop?: string;
  retry?: string;
  start?: string;
  markComplete?: string;
  release?: string;
  /** Stop of a stale node (`kill_node`). */
  kill?: string;
  /** Restart of the iteration (`restart_node`). */
  restart?: string;
}

/** What the node detail panel of a Run is given. */
export interface NodeDetailPanelProps {
  node: NodeState;
  runId: string;
  /** The Run's name, on the footer of an artifact opened from here. */
  runName?: string | null;
  isArchived?: boolean;
  /** The Run's status: a paused Run takes no completion until resumed. */
  runStatus?: RunStatus;
  nodeName?: string | null;
  initialTerminalExpanded?: boolean;
  provisioningRepository?: string;
  inheritedProvisioning?: ScopedProvisioningRules[];
  provisioningGitRef?: string;
  /** #723: the node's frozen « Orchestrator » toggle (ADR-0064). The
   *  Orchestration tab shows for any parent node (at least one child run); the
   *  toggle announces one, so its tab shows from the start (empty state). */
  isOrchestratorNode?: boolean;
  /** #723: open a child run listed in the Orchestration tab (App records the way back). */
  onOpenChildRun?: (childRunId: string) => void;
  /** #723: land on the Orchestration tab (the back arrow from a child run). */
  initialDetailTab?: "io" | "transcript" | "orchestration";
  /** ADR-0086: the node is an on-demand node — its Start is its ▶. */
  isOnDemand?: boolean;
  /** The Run's Coût de l'arbre, at the foot of the Orchestration tab. */
  runTreeCost?: TreeCost | null;
}
