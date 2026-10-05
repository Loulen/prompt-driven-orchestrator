import type { NodeType } from "../shared/generated/NodeType";
import type { EditorNodeDef } from "../pipeline/model/pipeline";

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
 * type carries no isolation at all: structural nodes never run in a worktree of
 * their own.
 */
const DEFAULT_ISOLATION: Record<NodeType, boolean | null> = {
  agent: true,
  script: false,
  // Legacy structural types, soft-warned by the daemon: no worktree either.
  switch: null,
  loop: null,
  start: null,
  end: null,
};

/** Whether the inspector offers this type a Workspace choice at all. */
export function carriesIsolation(type: NodeType): boolean {
  return DEFAULT_ISOLATION[type] !== null;
}

/**
 * The two places a NodeRun can work, named and described once (#653/#655). The
 * inspector renders them as radio choices and the library preview reuses the
 * label, so the editor speaks of a workspace in exactly one vocabulary — the
 * "new vocabulary everywhere" #655 asks for is a shared constant, not a
 * convention repeated per component.
 */
export const WORKSPACE_CHOICES = [
  {
    isolated: true,
    label: "Isolated worktree",
    hint: "a sub-worktree of the Node's own",
  },
  {
    isolated: false,
    label: "Run worktree",
    hint: "shared with the whole Run",
  },
] as const;

/** The label for a workspace, for a surface too small to show both choices. */
export function workspaceLabel(isolated: boolean): string {
  return WORKSPACE_CHOICES.find((c) => c.isolated === isolated)!.label;
}

/**
 * Isolation resolved from a stated value and a type, with the type having the
 * last word on whether there is a choice at all. `null` for a type that carries
 * none — the signal the serializer uses to leave the key off and the inspector
 * uses to hide the section.
 *
 * Takes the type as a bare string so the node library can call it too (#655): a
 * `LibraryEntry.type` crosses the wire untyped, and an entry saved for an
 * unknown type must resolve to `null` rather than to a guess.
 */
export function resolveIsolation(
  type: string,
  stated: boolean | null | undefined,
): boolean | null {
  const fallback = DEFAULT_ISOLATION[type as NodeType] ?? null;
  if (fallback === null) return null;
  return stated ?? fallback;
}

/**
 * A node's isolation as the document states it, falling back to its type's
 * default.
 */
export function nodeIsolation(node: EditorNodeDef): boolean | null {
  return resolveIsolation(node.type, node.isolated_worktree);
}

/**
 * Whether a node's NodeRun forks a worktree of its own — the canvas marker's
 * predicate. A type that carries no isolation reads `false`.
 */
export function isNodeIsolated(node: EditorNodeDef): boolean {
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
