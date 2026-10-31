/**
 * The working nodes of a Run as the row of the list draws them (#783).
 */

import type { NodeProgress } from "../../shared/generated/NodeProgress";
import type { RunListEntry } from "../../shared/generated/RunListEntry";

/** What stops when a Run does: a node archived while it ran no longer runs nor waits. */
const STOPPED_WITH_THE_RUN: ReadonlySet<NodeProgress["status"]> = new Set(["running", "awaiting_user", "waiting", "interrupted"]);

/** The nodes of `run` as its row draws them: those of an archived Run stopped with it. */
export function shownNodes(run: RunListEntry): NodeProgress[] {
  const nodes = run.nodes ?? [];
  if (run.status !== "archived") return nodes;
  return nodes.map((n) => (STOPPED_WITH_THE_RUN.has(n.status) ? { ...n, status: "stopped" } : n));
}
