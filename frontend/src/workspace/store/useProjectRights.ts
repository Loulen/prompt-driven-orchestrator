import { useEffect, useState } from "react";
import { askAuthorization } from "../../shared/authorization/api";
import type { Project } from "../model/project";
import {
  projectQuestions,
  projectRights,
  UNKNOWN_PROJECT_RIGHTS,
  type ProjectRights,
} from "../model/projectRights";

/** How many Operations one question may hold (the daemon's cap). */
const MAX_OPERATIONS = 500;

/**
 * What Settings › Projects lets the caller change, asked again whenever the
 * Projets change (a Projet created, deleted, or changed in another browser).
 * A failed question keeps the last answer: the daemon decides at the gesture.
 * Many Projets are asked about in several questions.
 */
export function useProjectRights(projects: Project[], active: boolean): ProjectRights {
  const [rights, setRights] = useState<ProjectRights>(UNKNOWN_PROJECT_RIGHTS);
  useEffect(() => {
    if (!active) return;
    let current = true;
    const questions = projectQuestions(projects);
    const parts = [];
    for (let i = 0; i < questions.length; i += MAX_OPERATIONS) parts.push(questions.slice(i, i + MAX_OPERATIONS));
    Promise.all(parts.map((part) => askAuthorization(part)))
      .then((decisions) => {
        if (current) setRights(projectRights(projects, decisions.flat()));
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
