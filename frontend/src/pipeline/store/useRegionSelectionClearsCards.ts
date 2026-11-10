import { useEffect, type Dispatch, type SetStateAction } from "react";
import type { Node } from "@xyflow/react";
import { useEditStore } from "./editStore";

/**
 * CORE-217: a loop region is not an xyflow selection (its nodes are inert), so
 * selecting one (its corner, `raise cap`, the Run info banner) would leave the
 * card selected before outlined. Clear xyflow's own selection then.
 */
export function useRegionSelectionClearsCards(setNodes: Dispatch<SetStateAction<Node[]>>) {
  const regionSelected = useEditStore((s) => s.selection.kind === "region");
  useEffect(() => {
    if (!regionSelected) return;
    setNodes((current) =>
      current.some((n) => n.selected) ? current.map((n) => (n.selected ? { ...n, selected: false } : n)) : current,
    );
  }, [regionSelected, setNodes]);
}
