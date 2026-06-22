export type RunStatus = "running" | "awaiting_user" | "completed" | "failed" | "skipped" | "halted" | "paused" | "archived";
export type NodeStatus = "pending" | "running" | "awaiting_user" | "completed" | "failed" | "stopped" | "stale";

export function isLiveRun(status: RunStatus): boolean {
  return status === "running" || status === "awaiting_user" || status === "paused";
}

/**
 * Live NodeRun-session count, the configured global cap, and the daemon
 * version, for the bottom status bar (#159 / ADR-0012, #139). Manager
 * sessions are excluded. `version` is absent until the daemon has responded.
 */
export interface DaemonStatus {
  live: number;
  cap: number;
  version?: string;
}
// `for-each` was removed (ADR-0011 / #151): a fan-out is now a `collection`
// loop region, not a node. The backend keeps the variant only to migrate old
// YAML into a region. `loop` was likewise removed in #171.
export type NodeType = "doc-only" | "code-mutating" | "start" | "end" | "merge";

export interface RunListEntry {
  run_id: string;
  pipeline_name: string;
  status: RunStatus;
  /**
   * Display-only "no forward progress" overlay (#180): true when the run has no
   * running/waiting node and a stale node. The dot renders amber and steady
   * even though `status` stays `"running"`. Derived server-side per read.
   */
  stalled?: boolean;
  started_at: string | null;
  name?: string | null;
  /** Provenance: the id of the Trigger that created this Run, if any (#160). */
  triggered_by?: string | null;
}

/**
 * A persisted Trigger (#160 / ADR-0012): a cron schedule bound to a run
 * template. Cron-only in this slice — `guard_command` is reserved for #161.
 */
export interface Trigger {
  id: string;
  name: string;
  pipeline_id: string;
  pipeline_name: string;
  target_repo?: string | null;
  source_branch?: string | null;
  input_template: string;
  variables: Record<string, unknown>;
  cron: string;
  guard_command?: string | null;
  overlap_policy: string;
  /** Bounded-`allow` ceiling (#239): max simultaneous live Runs; null = unbounded. */
  max_concurrent?: number | null;
  enabled: boolean;
  next_fire_at?: string | null;
  last_fired_at?: string | null;
  last_outcome?: string | null;
}

/** One audit row in a Trigger's fire history (`trigger_fires`). */
export interface TriggerFire {
  id: number;
  trigger_id: string;
  ts: string;
  outcome: string;
  reason?: string | null;
  run_id?: string | null;
  /**
   * Guard diagnostics on a `guard-exit-nonzero` row (#244): what the guard
   * printed and its exit status. Absent/null on every other outcome and on
   * legacy rows; each stream is tail-capped to 16 KB by the daemon.
   */
  guard_stdout?: string | null;
  guard_stderr?: string | null;
  guard_exit_code?: number | null;
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
  type:
    | "ready"
    | "heartbeat"
    | "event"
    | "pipeline_changed"
    | "trigger_created"
    | "trigger_fired"
    | "trigger_updated"
    | "trigger_deleted";
  event?: DaemonEvent;
  pipeline_id?: string;
  path?: string;
  ts?: string;
  /** Set on trigger_* messages (#160). */
  trigger_id?: string;
  outcome?: string;
  run_id?: string | null;
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
  /**
   * Whether a manual Run must supply a non-empty prompt (#158). Defaults to
   * `true` when absent; the New Run modal makes the prompt field optional when
   * this is `false`.
   */
  prompt_required?: boolean;
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

/** A pinned waypoint on a manually-routed edge — absolute canvas coordinates. */
export interface EdgeWaypoint {
  x: number;
  y: number;
}

/**
 * Edge routing mode (issue #154). `auto` edges store no waypoints — their
 * right-angle path is recomputed deterministically and re-routes on node move.
 * `manual` edges pin the route to persisted `waypoints`. Both `mode` and
 * `waypoints` are LAYOUT, not semantics: they persist in the pipeline file (so
 * routing travels when a workflow is shared) but are excluded from the semantic
 * pipeline-diff (see `comparablePipelineObject`).
 */
export type EdgeRouteMode = "auto" | "manual";

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
  /** Routing mode (#154). Absent ⇒ `auto`. */
  mode?: EdgeRouteMode | null;
  /** Pinned absolute waypoints (#154). Only meaningful when `mode === "manual"`. */
  waypoints?: EdgeWaypoint[] | null;
  /**
   * The target card side the incoming arrow anchors on (#168). When an edge is
   * dropped on an emergent node body (ADR-0011 / #149), the arrow anchors on the
   * side nearest the drop point rather than always the left. Like `mode`/
   * `waypoints` this is LAYOUT, not semantics: it persists in the file (so a
   * shared workflow keeps its arrow arrival sides) but is excluded from the
   * semantic pipeline-diff. Absent ⇒ left (legacy anchoring).
   */
  target_side?: PortSide | null;
}

/**
 * The kind of a named loop region (ADR-0011 / #148, #151). `bounded` regions
 * carry an iteration counter and a `max_iter`; they are born by auto-detection
 * of a cycle so no cycle is ever accidentally unbounded. `collection` regions
 * (ex-ForEach) carry an `over: <field>` driver and fan the member(s) out in
 * parallel, one lap per item, barriering on completion.
 */
export type LoopKind = "bounded" | "collection";

/**
 * A named loop region (ADR-0011 / #148, #151). Replaces the `loop` and `ForEach`
 * nodes: the loop is identified by `id`, its body is the explicit `members` list
 * (>= 1 node). A `bounded` region has a region-wide iteration counter keyed by
 * `id` and renders with a `↻ X/Y` header. A `collection` region fans `over` a
 * list and renders with a `⇉ N items` badge. The canvas draws either as a
 * translucent box (>= 2 members) or a compact badge (1 member).
 */
export interface LoopRegion {
  id: string;
  kind: LoopKind;
  members: string[];
  max_iter?: number | string | null;
  /** The frontmatter field a `collection` region fans out over (#151). */
  over?: string | null;
}

export interface PipelineDef {
  name: string;
  version?: string | null;
  variables: Record<string, VariableDef>;
  nodes: NodeDef[];
  edges: EdgeDef[];
  /** Named bounded loop regions (ADR-0011 / #148). Absent when there are none. */
  loops?: LoopRegion[];
  /**
   * Whether a manual Run must supply a non-empty prompt (#158). Defaults to
   * `true` (prompt mandatory) and is omitted from YAML in that case. When
   * `false`, a Run may start with empty input and a provided prompt is treated
   * as additional info.
   */
  prompt_required?: boolean;
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
