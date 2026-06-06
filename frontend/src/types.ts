export type RunStatus = "running" | "awaiting_user" | "completed" | "failed" | "halted" | "paused" | "archived";
export type NodeStatus = "pending" | "running" | "awaiting_user" | "completed" | "failed" | "stopped" | "stale";

export function isLiveRun(status: RunStatus): boolean {
  return status === "running" || status === "awaiting_user" || status === "paused";
}
export type NodeType = "doc-only" | "code-mutating" | "start" | "end" | "loop" | "for-each" | "merge";

export interface RunListEntry {
  run_id: string;
  pipeline_name: string;
  status: RunStatus;
  started_at: string | null;
  name?: string | null;
}

export interface IterationInfo {
  iter: number;
  status: NodeStatus;
  started_at: string | null;
  completed_at: string | null;
}

export interface NodeState {
  node_id: string;
  status: NodeStatus;
  iter: number;
  started_at: string | null;
  completed_at: string | null;
  failure_reason: string | null;
  iterations: IterationInfo[];
  frontmatter_retries?: number;
  frontmatter_violations?: Array<{ port: string; field: string; reason: string }>;
}

export interface EdgeInfo {
  source_node: string;
  source_port: string;
  target_node: string;
  target_port: string;
  halt_message?: string | null;
  when_clause?: Record<string, unknown> | null;
}

/**
 * Runtime trigger status for a single conditional edge (ADR-0011, #147).
 * Shown ONLY in the edge detail panel — never rendered on the canvas. Derived
 * from the run state; absent until the edge's source node has been evaluated.
 */
export interface EdgeTriggerStatus {
  fired: boolean;
  /** The clause's evaluated value rendered for display, e.g. `verdict = FAIL`. */
  last_value: string | null;
  evaluated_at: string | null;
  iter: number | null;
}

export interface PortBrief {
  name: string;
  side: PortSide;
  description?: string | null;
}

export interface NodeDefInfo {
  id: string;
  name?: string | null;
  node_type: NodeType;
  view_x: number | null;
  view_y: number | null;
  inputs: PortBrief[];
  outputs: PortBrief[];
}

export interface StartNodeInfo {
  input_path: string;
  started_at: string;
  target_node_ids: string[];
  // Filenames of images uploaded alongside the text prompt (stored in
  // `_input/`). Empty when the run was launched without images (issue #145).
  input_images: string[];
}

export interface EndPortStatus {
  port_name: string;
  status: string;
  reason: string | null;
  fired_at: string | null;
}

export interface EndNodeInfo {
  id: string;
  ports: EndPortStatus[];
}

export interface MergeResolverInfo {
  status: NodeStatus;
  conflicting_node_id: string;
  iter: number;
  session_name: string | null;
  started_at: string | null;
  completed_at: string | null;
  failure_reason: string | null;
}

export interface LoopStateInfo {
  loop_node_id: string;
  current_iter: number;
  max_iter: number;
  break_received: boolean;
  done: boolean;
}

export interface ForEachStateInfo {
  foreach_node_id: string;
  total_items: number;
  break_received: boolean;
  done: boolean;
}

export interface RunState {
  run_id: string;
  status: RunStatus;
  pipeline_name: string;
  name?: string | null;
  input: string | null;
  started_at: string | null;
  completed_at: string | null;
  nodes: Record<string, NodeState>;
  edges: EdgeInfo[];
  node_defs: NodeDefInfo[];
  start_node: StartNodeInfo | null;
  end_node: EndNodeInfo | null;
  merge_resolver: MergeResolverInfo | null;
  loop_states?: Record<string, LoopStateInfo>;
  foreach_states?: Record<string, ForEachStateInfo>;
  target_repo?: string | null;
  source_branch?: string | null;
}

export interface DaemonEvent {
  id: number | null;
  run_id: string;
  ts: string;
  kind: string;
  node_id: string | null;
  iter: number | null;
  payload: Record<string, unknown> | null;
}

export interface WsMessage {
  type: "ready" | "heartbeat" | "event" | "pipeline_changed";
  event?: DaemonEvent;
  pipeline_id?: string;
  path?: string;
  ts?: string;
}

export type EditScope = null | "run";

export interface PipelineVariableInfo {
  var_type: string;
  default: unknown;
}

// --- Edit mode types ---

export type PipelineScope = "repo" | "user" | "library";

export interface PipelineListEntry {
  id: string;
  name: string;
  scope: PipelineScope;
  path: string;
  node_count: number;
  modified: string | null;
  variables: Record<string, PipelineVariableInfo>;
  drifted?: boolean | null;
}

export type PortSide = "left" | "right" | "top" | "bottom";
export type PortType = "markdown" | "image" | "image_list";

export interface PortDef {
  name: string;
  repeated: boolean;
  side?: PortSide;
  port_type?: PortType;
  frontmatter?: Record<string, FrontmatterFieldDecl> | null;
  when?: Record<string, unknown> | null;
  description?: string | null;
}

export interface FrontmatterFieldDecl {
  type: string;
  allowed?: string[] | null;
}

export interface VariableDef {
  type: string;
  default: unknown;
}

export interface NodeDef {
  id: string;
  name?: string | null;
  type: NodeType;
  inputs: PortDef[];
  outputs: PortDef[];
  interactive: boolean;
  view?: { x: number; y: number } | null;
  max_iter?: number | string | null;
  over?: string | null;
}

export interface EdgeEndpoint {
  node: string;
  port: string;
}

export interface EdgeDef {
  source: EdgeEndpoint;
  target: EdgeEndpoint;
  reason?: string | null;
  /** Optional `when:` clause (ADR-0011): conditional routing on the edge. */
  when?: Record<string, unknown> | null;
  /** `else: true` marks a fallback edge (fires iff no sibling matched). */
  else?: boolean;
  /**
   * `repeated: true` marks an edge whose source artifact accumulates across
   * iterations (glob `iter-*`). Loop accumulation ("read all laps") lives on
   * the edge, not on a declared input port (ADR-0011 / #149).
   */
  repeated?: boolean;
}

export interface PipelineDef {
  name: string;
  version?: string | null;
  variables: Record<string, VariableDef>;
  nodes: NodeDef[];
  edges: EdgeDef[];
}

export interface PipelineDetail {
  id: string;
  scope: PipelineScope;
  path: string;
  yaml: string;
  pipeline: PipelineDef;
  prompts: Record<string, string>;
  diagnostics: string[];
}
