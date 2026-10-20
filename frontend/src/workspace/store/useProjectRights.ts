import { useEffect, useState } from "react";
import { askAuthorization } from "../../shared/authorization/api";
import type { Project } from "../model/project";
import {
  projectQuestions,
  projectRights,
  UNKNOWN_PROJECT_RIGHTS,
  type ProjectRights,
} from "../model/projectRights";

/**
 * What Settings › Projects lets the caller change, asked again whenever the
 * Projets change (a Projet created, deleted, or changed in another browser).
 * A failed question keeps the last answer: the daemon decides at the gesture.
 */
export function useProjectRights(projects: Project[], active: boolean): ProjectRights {
  const [rights, setRights] = useState<ProjectRights>(UNKNOWN_PROJECT_RIGHTS);
  useEffect(() => {
    if (!active) return;
    let current = true;
    askAuthorization(projectQuestions(projects))
      .then((decisions) => {
        if (current) setRights(projectRights(projects, decisions));
      })
      .catch(() => {
        // keep the last answer
      });
    return () => {
      current = false;
    };
  }, [projects, active]);
  return rights;
}
