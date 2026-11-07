import { useCallback, useEffect, useMemo } from "react";
import type { OpenIntent } from "../../run/components/NewRunModal";
import type { OpenPipeline, Selection } from "../../pipeline/store/editState";
import type { PipelineListEntry } from "../../shared/generated/PipelineListEntry";
import type { RunState } from "../../shared/generated/RunState";
import type { Trigger } from "../../shared/generated/Trigger";
import type { SettingsPosition } from "../../settings/components/settingsSurface/surfaceProps";
import { usePaletteHost, type PaletteHost, type PaletteSelection } from "./paletteHost";
import { whyNoRunAnywhere } from "../../run/model/runCreationRights";
import { useRunCreationRightsStore } from "../../run/store/runCreationRightsStore";

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

/** What the shell opens: a Run, a Trigger, the New Run form, Settings, Stats; and what it closes. */
interface ShellGestures extends Pick<PaletteHost, "runs" | "triggers" | "openRun" | "openTrigger"> {
  openNewRunModal: (intent: OpenIntent) => void;
  openSettings: (position?: SettingsPosition) => void;
  openStats: () => void;
  /** Closes the surface open under the top bar (CORE-183), then runs `then`. */
  closeSurfaces: (then?: () => void) => void;
}

/** Hand the command field the shell's lists, selection and gestures. */
export function usePaletteHostSync(shell: ShellGestures, selection: SelectionInput): void {
  const { selectedTrigger, selectedRun, editTab, canvas, pipelines } = selection;
  const selected = useMemo(
    () => paletteSelection({ selectedTrigger, selectedRun, editTab, canvas, pipelines }),
    [selectedTrigger, selectedRun, editTab, canvas, pipelines],
  );
  const { runs, triggers, openNewRunModal, closeSurfaces } = shell;
  // CORE-183: the command field stays usable over a surface; a Run or a Trigger it opens
  // shows in the Workbench, so the surface on screen gives way first (Settings may ask).
  const selectRun = shell.openRun;
  const selectTrigger = shell.openTrigger;
  const openRun = useCallback(
    (runId: string) => {
      closeSurfaces(() => selectRun(runId));
    },
    [closeSurfaces, selectRun],
  );
  const openTrigger = useCallback(
    (triggerId: string) => {
      closeSurfaces(() => selectTrigger(triggerId));
    },
    [closeSurfaces, selectTrigger],
  );
  const newRun = useCallback(() => openNewRunModal({ kind: "run" }), [openNewRunModal]);
  const newTrigger = useCallback(() => openNewRunModal({ kind: "new-trigger" }), [openNewRunModal]);
  const settings = shell.openSettings;
  const stats = shell.openStats;
  const openSettings = useCallback(() => settings(), [settings]);
  const openAppearance = useCallback(() => settings({ category: "appearance", section: "appearance" }), [settings]);
  const openStats = useCallback(() => stats(), [stats]);
  const noRunReason = useRunCreationRightsStore((s) => whyNoRunAnywhere(s.rights));
  const newRunRefused = noRunReason;
  useEffect(() => {
    usePaletteHost.getState().setHost({
      runs,
      triggers,
      selection: selected,
      openRun,
      openTrigger,
      newRun,
      newRunRefused,
      newTrigger,
      openSettings,
      openAppearance,
      openStats,
    });
  }, [runs, triggers, selected, openRun, openTrigger, newRun, newRunRefused, newTrigger, openSettings, openAppearance, openStats]);
}
