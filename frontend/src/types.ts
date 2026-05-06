export type RunStatus = "running" | "awaiting_user" | "completed" | "failed" | "halted" | "archived";
export type NodeStatus = "pending" | "running" | "awaiting_user" | "completed" | "failed";
export type NodeType = "doc-only" | "code-mutating";

export interface RunListEntry {
  run_id: string;
  pipeline_name: string;
  status: RunStatus;
  started_at: string | null;
}

export interface NodeState {
  node_id: string;
  status: NodeStatus;
  iter: number;
  started_at: string | null;
  completed_at: string | null;
  failure_reason: string | null;
}

export interface EdgeInfo {
  source_node: string;
  source_port: string;
  target_node: string;
  target_port: string;
  halt_message?: string | null;
  when_clause?: Record<string, unknown> | null;
}

export interface NodeDefInfo {
  id: string;
  node_type: NodeType;
  view_x: number | null;
  view_y: number | null;
  inputs: string[];
  outputs: string[];
}

export interface RunState {
  run_id: string;
  status: RunStatus;
  pipeline_name: string;
  input: string | null;
  started_at: string | null;
  completed_at: string | null;
  nodes: Record<string, NodeState>;
  edges: EdgeInfo[];
  node_defs: NodeDefInfo[];
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

export interface PortDef {
  name: string;
  repeated: boolean;
  frontmatter?: Record<string, FrontmatterFieldDecl> | null;
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
  type: NodeType;
  prompt_file?: string | null;
  inputs: PortDef[];
  outputs: PortDef[];
  interactive: boolean;
  view?: { x: number; y: number } | null;
}

export interface EdgeEndpoint {
  node: string;
  port: string;
}

export interface HaltTarget {
  message?: string | null;
}

export type EdgeTargetDef =
  | { node: string; port: string }
  | { halt: HaltTarget };

export interface EdgeDef {
  source: EdgeEndpoint;
  target: EdgeTargetDef;
  when?: Record<string, unknown> | null;
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
