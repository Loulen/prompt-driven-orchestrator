import { useCallback, useState } from "react";
import { fetchRuns } from "../run/api/runs";
import { fetchProjects } from "../workspace/api/projects";
import type { RunListEntry } from "../shared/generated/RunListEntry";
import type { Project } from "../workspace/model/project";

// The instance-wide lists the app shell holds: the Runs and the Projets.

export function useRuns() {
  const [runs, setRuns] = useState<RunListEntry[]>([]);
  // #823: « no Run at all » is half of the welcome-modal rule, and an empty array
  // before the first fetch means exactly the same thing as a Run-less instance.
  // Without this flag the modal would flash on every load, everywhere.
  const [loaded, setLoaded] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setRuns(await fetchRuns());
      setLoaded(true);
    } catch {
      // ignore
    }
  }, []);

  return { runs, runsLoaded: loaded, refresh };
}

// #552 — Projets, hydrated on mount and refreshed on a `project_changed` WS push
// (the daemon broadcasts one on every project mutation). Mirror of useTriggers.
export function useProjects() {
  const [projects, setProjects] = useState<Project[]>([]);

  const refresh = useCallback(async () => {
    try {
      setProjects(await fetchProjects());
    } catch {
      // ignore
    }
  }, []);

  return { projects, refresh };
}
