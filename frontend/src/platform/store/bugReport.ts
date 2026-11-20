import { create } from "zustand";

/** What the window starts with: the Run and the Pipeline of the selection. */
export interface BugReportPrefill {
  runId: string | null;
  /** A Pipeline's id, or the name a Run gives (matched against the list once read). */
  pipeline: string | null;
}

export const NO_PREFILL: BugReportPrefill = { runId: null, pipeline: null };

interface BugReportState {
  /** The window is open, with what it started from; `null` when closed. */
  opened: BugReportPrefill | null;
  /** The shell's selection, which an opening without a prefill starts from. */
  selection: BugReportPrefill;
  setSelection: (selection: BugReportPrefill) => void;
  /** Opens the Bug report window, on `prefill` or on the selection. */
  openBugReport: (prefill?: BugReportPrefill) => void;
  closeBugReport: () => void;
}

/**
 * The Bug report window (CONTEXT.md « Rapport de bug »): one for the app,
 * opened by Ctrl+K and by Settings › Diagnostics, mounted by the top bar.
 */
export const useBugReport = create<BugReportState>((set, get) => ({
  opened: null,
  selection: NO_PREFILL,
  setSelection: (selection) => set({ selection }),
  openBugReport: (prefill) => set({ opened: prefill ?? get().selection }),
  closeBugReport: () => set({ opened: null }),
}));

/** Opens the Bug report window on the shell's selection. */
export const openBugReport = (prefill?: BugReportPrefill) => useBugReport.getState().openBugReport(prefill);
