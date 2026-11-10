import { useCallback, useState } from "react";
import { fetchRuns } from "../run/api/runs";
import { fetchDeletedProjects, fetchProjects } from "../workspace/api/projects";
import { useRunCreationRightsStore } from "../run/store/runCreationRightsStore";
import type { RunListEntry } from "../shared/generated/RunListEntry";
import type { Project } from "../workspace/model/project";
import type { DeletedProject } from "../shared/generated/DeletedProject";

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
// CORE-5: each new list is also asked where a Run may be created.
export function useProjects() {
  const [projects, setProjects] = useState<Project[]>([]);
  // CORE-96: the deleted Projets, which the Runs launched in them keep.
  const [deleted, setDeleted] = useState<DeletedProject[]>([]);

  const refresh = useCallback(async () => {
    try {
      const fetched = await fetchProjects();
      setProjects(fetched);
      void useRunCreationRightsStore.getState().refresh(fetched);
    } catch {
      // ignore
    }
    try {
      setDeleted(await fetchDeletedProjects());
    } catch {
      // keep the last list: a Run of a Projet not listed falls back to its repository
    }
  }, []);

  return { projects, deleted, refresh };
}
