import type { Node } from "@xyflow/react";
import type { NodeStatus } from "../../shared/generated/NodeStatus";
import type { NodeType } from "../../shared/generated/NodeType";
import type { RunState } from "../../shared/generated/RunState";
import type { RunStatus } from "../../shared/generated/RunStatus";
import { isNodeIsolated } from "../../lib/nodeIsolation";
import { fallbackNodeSpot, fallbackNoteSpot } from "../../lib/nodePlacement";
import type { EditorPipelineDef } from "../model/pipeline";
import { deriveLoopRegions } from "./loopRegionDerivation";
import { regionCostText } from "./loopRegionCost";
import type { LoopBadge } from "./EditNode";
import { nodeAgentLabel } from "../model/nodeAgent";
import { profileLabel } from "../../settings/model/profileNames";
import type { AgentProfile } from "../../shared/generated/AgentProfile";

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
 * A running run that waits at `End` for its started nodes (ADR-0086) has
 * reached it too: its banner says so, and the card must not contradict it.
 */
export function markerReached(
  nodeType: NodeType,
  runState: RunState | null | undefined,
): boolean {
  if (nodeType !== "start" && nodeType !== "end") return false;
  if (runState == null) return false;
  return runReachedEnd(runState.status) || (runState.waiting_for_started_nodes ?? 0) > 0;
}

export function deriveEditNodes(
  pipeline: EditorPipelineDef,
  runState: RunState | null | undefined,
  agentProfileIds: ReadonlySet<string> = new Set(),
  agentProfiles: readonly AgentProfile[] = [],
  /** The Run is edited for itself: a loop badge's cap reads the tab, edits included. */
  editingRun = false,
): Node[] {
  // A single-member loop region renders as a compact badge on the member's card
  // instead of a box (ADR-0011 / #148, #151; the LoopRegion type's "compact
  // badge (1 member)"). A `collection` reads `⇉ <fan-out>`; a single-member
  // `bounded` region (a self-loop, or a multi-member loop reduced to one present
  // member, e.g. after a member is deleted — #173) reads `↻ <counter>`,
  // mirroring the box header — so a one-member loop is never invisible (it draws
  // no box). Keyed by member id; the kind drives the glyph and the title.
  const loopBadgeByMember = new Map<string, LoopBadge>();
  for (const region of deriveLoopRegions(pipeline, runState, undefined, editingRun)) {
    if (region.badgeMemberId == null) continue;
    const symbol = region.kind === "collection" ? "⇉" : "↻";
    const costText = regionCostText(region);
    loopBadgeByMember.set(region.badgeMemberId, {
      text: `${symbol} ${region.counterText}`,
      kind: region.kind,
      // The Coût de boucle follows the counter, as in a region's corner.
      ...(costText != null ? { costText } : {}),
      ...(region.cost ? { costTitle: region.cost.title } : {}),
      ...(region.costWarn ? { costWarn: true } : {}),
      ...(region.budgetReached ? { budgetReached: true } : {}),
    });
  }
  return pipeline.nodes.map((n, i) => {
    const status = statusForNode(n.id, runState);
    const loopBadge = loopBadgeByMember.get(n.id);
    // Shared with the drop search (`lib/nodePlacement.ts`): a node the document
    // gives no position for is on screen all the same, and a new card must not
    // land on it.
    const fallback = fallbackNodeSpot(i);
    const nodeRun = runState?.nodes?.[n.id];
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
        inputs: n.inputs.map((p) => ({ name: p.name, description: p.description })),
        // The card lists each output with its type's icon.
        outputs: n.outputs.map((p) => ({ name: p.name, description: p.description, port_type: p.port_type ?? "markdown" })),
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
        // ADR-0086: the « on demand » option, as the document says — the
        // runtime reads the Run's pipeline, so an edit for this run counts. An
        // on-demand node the Run never launched is drawn greyed with its ▶.
        onDemand: n.on_demand ?? false,
        inRun: runState != null,
        // The card's meta grid: what runs the node, and in a Run its
        // iteration, its time and its cost.
        agent: nodeAgentLabel(n, agentProfiles, nodeRun?.harness, profileLabel),
        run: nodeRun
          ? {
              iter: nodeRun.iter,
              iterations: nodeRun.iterations?.length ?? 0,
              startedAt: nodeRun.started_at,
              completedAt: nodeRun.completed_at,
              cost: nodeRun.cost ?? null,
            }
          : undefined,
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
    position: note.view ? { x: note.view.x, y: note.view.y } : fallbackNoteSpot(i),
    data: { noteId: note.id, content: note.content },
    draggable: true,
    selectable: true,
    connectable: false,
  }));
}
