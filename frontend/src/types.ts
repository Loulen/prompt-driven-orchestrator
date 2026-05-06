export type RunStatus = "running" | "completed" | "failed";
export type NodeStatus = "pending" | "running" | "completed" | "failed";
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
  type: "ready" | "heartbeat" | "event";
  event?: DaemonEvent;
  ts?: string;
}
