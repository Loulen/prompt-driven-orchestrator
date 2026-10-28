import { useEffect, useMemo } from "react";
import type { OpenPipeline, Selection } from "../../pipeline/store/editState";
import type { PipelineListEntry } from "../../shared/generated/PipelineListEntry";
import type { RunState } from "../../shared/generated/RunState";
import type { Trigger } from "../../shared/generated/Trigger";
import { usePaletteHost, type PaletteHost, type PaletteSelection } from "./paletteHost";

interface SelectionInput {
  selectedTrigger: Trigger | null;
  selectedRun: RunState | null;
  /** The editor tab on screen. */
  editTab: OpenPipeline | undefined;
  /** The canvas's selection. */
  canvas: Selection;
  pipelines: PipelineListEntry[];
}

/**
 * What the command field acts on: the selected Trigger; else the node selected
 * on the canvas of the Run on screen, or that Run; else the Run selected in
 * the list; else the Pipeline of the editor tab.
 */
export function paletteSelection({ selectedTrigger, selectedRun, editTab, canvas, pipelines }: SelectionInput): PaletteSelection | null {
  if (selectedTrigger) return { kind: "trigger", trigger: selectedTrigger };
  if (selectedRun) {
    const run = selectedRun;
    const onScreen = editTab?.runId === run.run_id;
    const node = onScreen && canvas.kind === "node" && canvas.id ? run.nodes[canvas.id] : undefined;
    if (node) {
      const def = run.node_defs.find((d) => d.id === node.node_id);
      return {
        kind: "node",
        run: { id: run.run_id, status: run.status },
        node: { id: node.node_id, name: def?.name ?? null, type: def?.node_type ?? null, status: node.status, iter: node.iter },
      };
    }
    return {
      kind: "run",
      run: { id: run.run_id, name: run.name ?? null, pipeline: run.pipeline_name, status: run.status, attributes: run.attributes ?? {} },
    };
  }
  if (editTab && !editTab.runId && !editTab.version) {
    const entry = pipelines.find((p) => p.id === editTab.id);
    return {
      kind: "pipeline",
      pipeline: { id: editTab.id, name: entry?.name ?? editTab.pipeline.name, attributes: entry?.attributes ?? {} },
    };
  }
  return null;
}

/** Hand the command field the shell's lists, selection and gestures. */
export function usePaletteHostSync(
  host: Omit<PaletteHost, "selection">,
  selection: SelectionInput,
): void {
  const { selectedTrigger, selectedRun, editTab, canvas, pipelines } = selection;
  const selected = useMemo(
    () => paletteSelection({ selectedTrigger, selectedRun, editTab, canvas, pipelines }),
    [selectedTrigger, selectedRun, editTab, canvas, pipelines],
  );
  const { runs, triggers, openRun, openTrigger, newRun, newTrigger, openSettings, openStats } = host;
  useEffect(() => {
    usePaletteHost.getState().setHost({
      runs,
      triggers,
      selection: selected,
      openRun,
      openTrigger,
      newRun,
      newTrigger,
      openSettings,
      openStats,
    });
  }, [runs, triggers, selected, openRun, openTrigger, newRun, newTrigger, openSettings, openStats]);
}
