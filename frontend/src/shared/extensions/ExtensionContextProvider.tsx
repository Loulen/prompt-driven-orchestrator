import { useMemo, type ReactNode } from "react";
import type { RunListEntry } from "../generated/RunListEntry";
import type { RunState } from "../generated/RunState";
import type { Project } from "../../workspace/model/project";
import {
  BaseContextValue,
  NOTHING_SELECTED,
  projectOfRun,
  runContext,
  type BaseContext,
} from "./context";

/** Provide the selected Run and its Projet to every contribution below. */
export default function ExtensionContextProvider({
  selectedRun,
  runs,
  projects,
  children,
}: {
  selectedRun: RunState | null;
  runs: RunListEntry[];
  projects: Project[];
  children: ReactNode;
}) {
  const value = useMemo<BaseContext>(() => {
    if (!selectedRun) return NOTHING_SELECTED;
    return {
      run: runContext(selectedRun),
      project: projectOfRun(selectedRun.run_id, runs, projects),
    };
  }, [selectedRun, runs, projects]);
  return <BaseContextValue.Provider value={value}>{children}</BaseContextValue.Provider>;
}

