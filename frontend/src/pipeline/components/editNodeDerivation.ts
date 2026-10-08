import type { Node } from "@xyflow/react";
import type { LoopRegion } from "../../shared/generated/LoopRegion";
import type { NodeStatus } from "../../shared/generated/NodeStatus";
import type { NodeType } from "../../shared/generated/NodeType";
import type { RunState } from "../../shared/generated/RunState";
import type { RunStatus } from "../../shared/generated/RunStatus";
import { isNodeIsolated } from "../../lib/nodeIsolation";
import { fallbackNodeSpot } from "../../lib/nodePlacement";
import type { EditorPipelineDef } from "../model/pipeline";
import { deriveLoopRegions } from "./loopRegionDerivation";

/**
 * A run "reaches its end" when it terminates successfully (`completed`). At
 * that point the start/end marker nodes pick up the same green "done" signal
 * that standard nodes already show on completion. Failed/halted runs are not
 * treated as reached — the end node keeps its neutral "blocked" colour.
 */
export function runReachedEnd(status: RunStatus): boolean {
  return status === "completed";
}

export function statusForNode(
  nodeId: string,
  runState: RunState | null | undefined,
): NodeStatus {
  return runState?.nodes[nodeId]?.status ?? "pending";
}

/**
 * Whether a start/end marker should show the green "reached the end" cadre in
 * the inline run view (`EditCanvas`), preserving the intent of issue #105 in
 * the view users actually see. Only the start/end markers carry this — regular
 * nodes always report their own live status. It is gated on a live run: editing
 * a library/template pipeline (no run state) never colours the markers.
 */
export function markerReached(
  nodeType: NodeType,
  runState: RunState | null | undefined,
): boolean {
  if (nodeType !== "start" && nodeType !== "end") return false;
  return runState != null && runReachedEnd(runState.status);
}

export function deriveEditNodes(
  pipeline: EditorPipelineDef,
  runState: RunState | null | undefined,
  agentProfileIds: ReadonlySet<string> = new Set(),
): Node[] {
  // A single-member loop region renders as a compact badge on the member's card
  // instead of a box (ADR-0011 / #148, #151; the LoopRegion type's "compact
  // badge (1 member)"). A `collection` reads `⇉ <fan-out>`; a single-member
  // `bounded` region (a self-loop, or a multi-member loop reduced to one present
  // member, e.g. after a member is deleted — #173) reads `↻ <counter>`,
  // mirroring the box header — so a one-member loop is never invisible (it draws
  // no box). Keyed by member id; the kind drives the glyph and the title.
  const loopBadgeByMember = new Map<string, { text: string; kind: LoopRegion["kind"] }>();
  for (const region of deriveLoopRegions(pipeline, runState)) {
    if (region.badgeMemberId == null) continue;
    const symbol = region.kind === "collection" ? "⇉" : "↻";
    loopBadgeByMember.set(region.badgeMemberId, {
      text: `${symbol} ${region.counterText}`,
      kind: region.kind,
    });
  }
  return pipeline.nodes.map((n, i) => {
    const status = statusForNode(n.id, runState);
    const loopBadge = loopBadgeByMember.get(n.id);
    // Shared with the drop search (`lib/nodePlacement.ts`): a node the document
    // gives no position for is on screen all the same, and a new card must not
    // land on it.
    const fallback = fallbackNodeSpot(i);
    return {
      id: n.id,
      type: "edit",
      position: {
        x: n.view?.x ?? fallback.x,
        y: n.view?.y ?? fallback.y,
      },
      data: {
        label: n.name ?? n.id,
        nodeId: n.id,
        nodeType: n.type,
        // #653/ADR-0060: the canvas marker's input. On a live Run the FROZEN
        // answer wins — the marker has to say where the NodeRun works, and an
        // edit that has not launched yet moved the document, not the session.
        isolated: runState?.nodes?.[n.id]?.isolated_worktree ?? isNodeIsolated(n),
        status,
        reached: markerReached(n.type, runState),
        // Input images uploaded with the run surface on the start marker only
        // (issue #145). They come from the projected run state, so template
        // editing (no run state) leaves the start node image-free.
        inputImages:
          n.type === "start" ? (runState?.start_node?.input_images ?? []) : undefined,
        inputs: n.inputs.map((p) => ({ name: p.name, side: p.side ?? "left", description: p.description })),
        outputs: n.outputs.map((p) => ({
          name: p.name,
          side: p.side ?? "right",
          description: p.description,
          ...(p.intermediate ? { intermediate: true } : {}),
        })),
        interactive: n.interactive,
        agentMode:
          n.agent_choice?.mode === "profile"
            ? agentProfileIds.has(n.agent_choice.profile_id) ? "profile" : "broken"
            : n.agent_choice?.mode === "custom" ? "custom" : "inherit",
        // Compact badge for the single member of a loop region: `⇉ ...` for a
        // collection (#151) or `↻ ...` for a single-member bounded loop (#173).
        // Absent on non-member nodes and multi-member regions (boxed instead).
        loopBadge,
        // #723/ADR-0064: the node's « Orchestrator » toggle. On a live Run the
        // FROZEN answer (the snapshot's NodeDefInfo) wins, exactly like
        // `isolated` above; on a template the document says. The canvas pastilles
        // (EditCanvas) attach the child counters to cards where this is true.
        orchestrator:
          runState?.node_defs?.find((d) => d.id === n.id)?.orchestrator ?? n.orchestrator ?? false,
      },
    };
  });
}

/**
 * Builds the xyflow `note` nodes for every inert canvas note (#307 / ADR-0018).
 * A note is draggable and selectable but NOT connectable — it renders no handle,
 * so no edge can ever attach. Missing `view` falls back to a default spot, like
 * a node with no persisted position. Notes are a canvas concept, never part of
 * `pipeline.nodes`; they carry no run status and never colour.
 */
export function buildNoteNodes(pipeline: EditorPipelineDef): Node[] {
  return (pipeline.notes ?? []).map((note, i) => ({
    id: note.id,
    type: "note",
    position: {
      x: note.view?.x ?? 240,
      y: note.view?.y ?? 80 + i * 120,
    },
    data: { noteId: note.id, content: note.content },
    draggable: true,
    selectable: true,
    connectable: false,
  }));
}
