import { useEditStore } from "../stores/editStore";
import { useWiringStore } from "../stores/wiringStore";
import { gridStep, resolveGridSize, type GridSize } from "../lib/wiringGrid";

/**
 * The wiring-grid size in effect on the active canvas (#877 / ADR-0076): the
 * active pipeline's own `grid_size`, else the reader's global default. Every
 * wiring consumer (trace snap, segment drag, landing legs, rim anchor, grid
 * overlay, decorative background) reads the step through here, so a change in
 * the pipeline pane or in Settings re-steps the whole canvas at once.
 */
export function useWiringGridSize(): GridSize {
  const pipelineSize = useEditStore(
    (s) => s.openTabs.find((t) => t.id === s.activeTabId)?.pipeline.grid_size,
  );
  const globalSize = useWiringStore((s) => s.defaultGridSize);
  return resolveGridSize(pipelineSize, globalSize);
}

/** The step, in flow px, of `useWiringGridSize()`. */
export function useWiringGridStep(): number {
  return gridStep(useWiringGridSize());
}
