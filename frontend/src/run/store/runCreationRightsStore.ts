import { useEffect } from "react";
import { create } from "zustand";
import { askAuthorization } from "../../shared/authorization/api";
import type { Project } from "../../workspace/model/project";
import {
  runCreationQuestions,
  runCreationRights,
  UNKNOWN_RIGHTS,
  type RunCreationRights,
} from "../model/runCreationRights";

/**
 * Where the caller may create a Run (CORE-5), shared by the « New Run » button
 * and the New Run form. Asked again whenever the Projets change, and each time
 * the form opens (a right may have changed elsewhere, in an extension), and
 * while « New Run » is disabled (`useRecheckRunCreationRights`). A
 * failed question keeps the last answer: the daemon still decides at Launch.
 */
interface RunCreationRightsState {
  rights: RunCreationRights;
  /** The Projets the last question was about. */
  projects: Project[];
  /** Ask about `projects` (the last ones when omitted). */
  refresh: (projects?: Project[]) => Promise<void>;
}

let asked = 0;

export const useRunCreationRightsStore = create<RunCreationRightsState>((set, get) => ({
  rights: UNKNOWN_RIGHTS,
  projects: [],
  refresh: async (projects) => {
    const about = projects ?? get().projects;
    if (projects) set({ projects });
    const ticket = ++asked;
    try {
      const decisions = await askAuthorization(runCreationQuestions(about));
      // A later question won the race: its answer is the one to keep.
      if (ticket === asked) set({ rights: runCreationRights(about, decisions) });
    } catch {
      // keep the last answer
    }
  },
}));

/** How often a « New Run » disabled for want of rights asks again. */
export const RECHECK_RIGHTS_EVERY_MS = 15_000;

/**
 * While `denied`, ask again every few seconds and whenever the window comes
 * back: once disabled, « New Run » can no longer open the form that would
 * ask, so a right given back elsewhere would otherwise wait for a reload.
 */
export function useRecheckRunCreationRights(denied: boolean): void {
  useEffect(() => {
    if (!denied) return;
    const recheck = () => void useRunCreationRightsStore.getState().refresh();
    const whenVisible = () => {
      if (document.visibilityState === "visible") recheck();
    };
    const timer = window.setInterval(recheck, RECHECK_RIGHTS_EVERY_MS);
    window.addEventListener("focus", recheck);
    document.addEventListener("visibilitychange", whenVisible);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener("focus", recheck);
      document.removeEventListener("visibilitychange", whenVisible);
    };
  }, [denied]);
}
