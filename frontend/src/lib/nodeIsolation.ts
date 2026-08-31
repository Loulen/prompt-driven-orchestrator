import type { NodeDef, NodeType } from "../types";

/**
 * #653 / ADR-0060 — the one owner of "where does this node work?" on the client.
 *
 * The retired non-isolated / isolated types named an *effect* while the
 * runtime only ever read them as a working-directory decision. `agent` names the
 * role; `isolated_worktree` names the directory. Everything on this axis — the
 * inspector's Workspace section, the canvas marker, the serializer's
 * unconditional emit, the new-node defaults — reads it from here, so the editor
 * and the daemon cannot drift apart on what a silent document means.
 */

/**
 * The isolation a type carries when the document says nothing. `null` means the
 * type carries no isolation at all: `merge` is isolated by construction (it
 * exposes no control — a Merge that could share a tree would have nothing to
 * merge), and structural nodes never run in a worktree of their own.
 */
const DEFAULT_ISOLATION: Record<NodeType, boolean | null> = {
  agent: true,
  script: false,
  merge: null,
  start: null,
  end: null,
};

/** Whether the inspector offers this type a Workspace choice at all. */
export function carriesIsolation(type: NodeType): boolean {
  return DEFAULT_ISOLATION[type] !== null;
}

/**
 * A node's isolation as the document states it, falling back to its type's
 * default. `null` for a type that carries none — the signal the serializer uses
 * to leave the key off and the inspector uses to hide the section.
 */
export function nodeIsolation(node: NodeDef): boolean | null {
  const fallback = DEFAULT_ISOLATION[node.type] ?? null;
  if (fallback === null) return null;
  return node.isolated_worktree ?? fallback;
}

/**
 * Whether a node's NodeRun forks a worktree of its own — the canvas marker's
 * predicate. Unlike [`nodeIsolation`] this answers for `merge` too, which is
 * isolated without a line to read: the marker has to say so, or a Merge would
 * look *less* isolated than the Agent feeding it.
 */
export function isNodeIsolated(node: NodeDef): boolean {
  if (node.type === "merge") return true;
  return nodeIsolation(node) === true;
}

/**
 * The working directory the NodeRun will actually get, relative to the repo
 * root. Displayed under the Workspace choice so the author reads the path
 * instead of deducing it. `<run>` and `<n>` stay placeholders in the editor —
 * neither the Run nor the iteration exists yet.
 */
export function worktreePathFor(nodeId: string, isolated: boolean): string {
  return isolated
    ? `.pdo/runs/<run>/nodes/${nodeId}/iter-<n>`
    : `.pdo/runs/<run>/worktree`;
}
