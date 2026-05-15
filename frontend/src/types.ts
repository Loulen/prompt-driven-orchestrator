export type RunStatus = "running" | "awaiting_user" | "completed" | "failed" | "halted" | "paused" | "archived";
export type NodeStatus = "pending" | "running" | "awaiting_user" | "completed" | "failed" | "stopped" | "stale";

export function isLiveRun(status: RunStatus): boolean {
  return status === "running" || status === "awaiting_user" || status === "paused";
}
export type NodeType = "doc-only" | "code-mutating" | "start" | "end" | "switch" | "loop" | "for-each" | "merge";

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

export interface SwitchStateInfo {
  switch_node_id: string;
  chosen_branch: string;
  evaluated_at: string;
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
  switch_states?: Record<string, SwitchStateInfo>;
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

export type PipelineScope = "repo" | "user";

export interface PipelineListEntry {
  id: string;
  name: string;
  scope: PipelineScope;
  path: string;
  node_count: number;
  modified: string | null;
  variables: Record<string, PipelineVariableInfo>;
}

export type PortSide = "left" | "right" | "top" | "bottom";

export interface PortDef {
  name: string;
  repeated: boolean;
  side?: PortSide;
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
