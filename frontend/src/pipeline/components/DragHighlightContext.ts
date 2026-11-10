import { createContext, useContext } from "react";

const DragHighlightCtx = createContext<string | null>(null);

export const DragHighlightProvider = DragHighlightCtx.Provider;

export function useIsDropTarget(nodeId: string): boolean {
  return useContext(DragHighlightCtx) === nodeId;
}
