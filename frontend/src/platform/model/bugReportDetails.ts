/**
 * The details a Rapport de bug joins (CONTEXT.md): a Run's and a Pipeline's,
 * as markdown. A Run gives its status, each node's status, the failure reasons
 * and the durations, never an artifact nor an agent's output; a Pipeline its
 * structure (nodes, types, harnesses, isolation, edges), never a prompt. The
 * prompts have their own section, joined only where the destination accepts
 * them.
 */

import type { AgentChoice } from "../../shared/generated/AgentChoice";
import type { EdgeDef } from "../../shared/generated/EdgeDef";
import type { NodeType } from "../../shared/generated/NodeType";
import type { RunState } from "../../shared/generated/RunState";
import { formatDuration } from "../../run/model/runDuration";

/** What the run details read of `GET /runs/{id}`. */
export type RunFacts = Pick<
  RunState,
  "run_id" | "name" | "pipeline_name" | "status" | "started_at" | "completed_at" | "failure_reason" | "awaiting_reason" | "nodes" | "node_defs"
>;

/** What the pipeline details read of a Pipeline: its structure, its prompts apart. */
export interface PipelineFacts {
  name: string;
  nodes: {
    id: string;
    name: string | null;
    type: NodeType;
    pin_harness?: string | null;
    agent_choice?: AgentChoice | null;
    isolated_worktree?: boolean | null;
  }[];
  edges: Pick<EdgeDef, "source" | "target" | "when" | "else">[];
  /** Each node's prompt, by node id. */
  prompts: Record<string, string>;
}

/** The longest failure reason a row keeps. */
const REASON_CAP = 300;

const count = (n: number, noun: string) => `${n} ${noun}${n === 1 ? "" : "s"}`;

/** The line under the details when nodes were dropped to fit. */
export const truncatedLine = (omitted: number) => `(truncated, ${count(omitted, "node")} omitted)`;

/** A table cell: one line, no pipe that would end it. */
function cell(text: string | null | undefined): string {
  const flat = (text ?? "").replace(/\s*\n\s*/g, " ").replace(/\|/g, "\\|").trim();
  return flat.length > REASON_CAP ? `${flat.slice(0, REASON_CAP - 1)}…` : flat;
}

function table(head: string[], rows: string[][]): string[] {
  const line = (cells: string[]) => `| ${cells.join(" | ")} |`;
  return [line(head), line(head.map(() => "---")), ...rows.map(line)];
}

function span(start: string | null | undefined, end: string | null | undefined, now: number): string {
  if (!start) return "";
  const from = Date.parse(start);
  const to = end ? Date.parse(end) : now;
  return formatDuration(to - from) ?? "";
}

const STRUCTURAL = new Set<NodeType>(["start", "end"]);

/**
 * A Run as the report says it: its id, name, status and duration, its failure
 * or waiting reason, then one row per node (the Start and End aside): name,
 * type, status, duration, failure.
 */
export function runDetails(run: RunFacts, now: number): string {
  const head = [`Run ${run.run_id}`, run.name, run.status, span(run.started_at, run.completed_at, now)];
  const lines = [head.filter(Boolean).join(" · ")];
  if (run.failure_reason) lines.push(`Failure: ${cell(run.failure_reason)}`);
  if (run.awaiting_reason) lines.push(`Waiting: ${cell(run.awaiting_reason)}`);
  const rows = run.node_defs
    .filter((def) => !STRUCTURAL.has(def.node_type))
    .map((def) => {
      const state = run.nodes[def.id];
      return [
        cell(def.name || def.id),
        def.node_type,
        state?.status ?? "pending",
        state ? span(state.started_at, state.completed_at, now) : "",
        cell(state?.failure_reason ?? state?.skip_reason),
      ];
    });
  if (rows.length > 0) lines.push("", ...table(["Node", "Type", "Status", "Duration", "Failure"], rows));
  return lines.join("\n");
}

function harnessOf(node: PipelineFacts["nodes"][number]): string {
  if (node.type !== "agent") return "";
  const choice = node.agent_choice;
  if (choice?.mode === "custom") return [choice.harness, choice.model].filter(Boolean).join(" ");
  if (choice?.mode === "profile") return `profile ${choice.profile_id}`;
  return node.pin_harness ?? "default";
}

function isolationOf(node: PipelineFacts["nodes"][number]): string {
  if (node.isolated_worktree == null) return node.type === "agent" ? "default" : "";
  return node.isolated_worktree ? "isolated" : "shared";
}

/**
 * A Pipeline's structure: its name and size, one row per node (name, type,
 * harness, isolation), then its edges. No prompt.
 */
export function pipelineDetails(pipeline: PipelineFacts): string {
  const names = new Map(pipeline.nodes.map((n) => [n.id, n.name || n.id]));
  const name = (id: string) => names.get(id) ?? id;
  const lines = [`Pipeline ${pipeline.name} · ${count(pipeline.nodes.length, "node")} · ${count(pipeline.edges.length, "edge")}`];
  const rows = pipeline.nodes.map((n) => [cell(name(n.id)), n.type, cell(harnessOf(n)), isolationOf(n)]);
  if (rows.length > 0) lines.push("", ...table(["Node", "Type", "Harness", "Isolation"], rows));
  if (pipeline.edges.length > 0) {
    lines.push("", "Edges:");
    for (const edge of pipeline.edges) {
      const from = edge.source.port ?? edge.source.ports?.join(",") ?? "";
      const guard = edge.else ? " (else)" : edge.when ? " (conditional)" : "";
      lines.push(`- ${name(edge.source.node)}${from ? `.${from}` : ""} -> ${name(edge.target.node)}.${edge.target.port}${guard}`);
    }
  }
  return lines.join("\n");
}

/** The prompts of a Pipeline, node by node, for a destination that accepts them. */
export function promptDetails(pipeline: PipelineFacts): string {
  const names = new Map(pipeline.nodes.map((n) => [n.id, n.name || n.id]));
  return Object.entries(pipeline.prompts)
    .filter(([, prompt]) => prompt.trim() !== "")
    .map(([id, prompt]) => `#### ${names.get(id) ?? id}\n${prompt.trim()}`)
    .join("\n\n");
}
