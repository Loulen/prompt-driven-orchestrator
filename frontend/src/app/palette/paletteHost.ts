import { create } from "zustand";
import type { NodeStatus } from "../../shared/generated/NodeStatus";
import type { RunListEntry } from "../../shared/generated/RunListEntry";
import type { RunStatus } from "../../shared/generated/RunStatus";
import type { Trigger } from "../../shared/generated/Trigger";

/**
 * The item selected in the interface, as the command field acts on it: a
 * Run, a node of the Run on screen, a Trigger, or the Pipeline of the editor
 * tab.
 */
export type PaletteSelection =
  | { kind: "run"; run: { id: string; name: string | null; pipeline: string; status: RunStatus; attributes: Record<string, string> } }
  | {
      kind: "node";
      run: { id: string; status: RunStatus };
      node: { id: string; name: string | null; type: string | null; status: NodeStatus; iter: number };
    }
  | { kind: "trigger"; trigger: Trigger }
  | { kind: "pipeline"; pipeline: { id: string; name: string; attributes: Record<string, string> } };

/**
 * What the app shell hands the command field: its lists, the selection, and
 * the gestures it opens. The palette reads it when it opens; without a shell
 * (a test of the top bar alone) it lists the commands only.
 */
export interface PaletteHost {
  runs: RunListEntry[];
  triggers: Trigger[];
  selection: PaletteSelection | null;
  openRun: (runId: string) => void;
  openTrigger: (triggerId: string) => void;
  newRun?: () => void;
  newTrigger?: () => void;
  openSettings?: () => void;
  openStats?: () => void;
}

const NONE: PaletteHost = {
  runs: [],
  triggers: [],
  selection: null,
  openRun: () => {},
  openTrigger: () => {},
};

export const usePaletteHost = create<{ host: PaletteHost; setHost: (host: PaletteHost) => void }>((set) => ({
  host: NONE,
  setHost: (host) => set({ host }),
}));
