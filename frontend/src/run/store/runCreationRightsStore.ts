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
 * the form opens (a right may have changed elsewhere, in an extension). A
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
