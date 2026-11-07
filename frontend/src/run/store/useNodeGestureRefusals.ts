import { useMemo } from "react";
import type { NodeGestureRefusals } from "../components/nodeDetail/types";
import { useWhyNotOnNode } from "./gestureRightsStore";

/**
 * The gestures on the node `nodeId` of `runId` the caller may not make, as
 * the view of the Run asked them once when it opened (CORE-42); CORE-205: the
 * refused ones stay, greyed, their tooltip saying the refusal's sentence.
 */
export function useNodeGestureRefusals(runId: string, nodeId: string): NodeGestureRefusals {
  const whyNot = useWhyNotOnNode(runId, nodeId);
  return useMemo(
    () => ({
      stop: whyNot("stop"),
      retry: whyNot("retry"),
      start: whyNot("start"),
      markComplete: whyNot("mark_node_done"),
      release: whyNot("release_completion"),
      kill: whyNot("kill_node"),
      restart: whyNot("restart_node"),
    }),
    [whyNot],
  );
}
