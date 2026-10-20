/**
 * Projets over HTTP (ADR-0046): the group-header pencil's CRUD, and the middle
 * tier of the harness and skills axes.
 */

import { request } from "../../shared/http/client";
import type { AgentChoice } from "../../shared/generated/AgentChoice";
import type { DeletedProject } from "../../shared/generated/DeletedProject";
import type { SkillRef } from "../../shared/generated/SkillRef";
import type { Project } from "../model/project";

export function fetchProjects(): Promise<Project[]> {
  return request<Project[]>("GET", "/projects");
}

/** The deleted Projets the caller may read (CORE-96): the Runs launched in one keep it. */
export function fetchDeletedProjects(): Promise<DeletedProject[]> {
  return request<DeletedProject[]>("GET", "/projects/deleted");
}

/**
 * Name a new Projet (`POST /projects`). The initial `members` travel with the
 * name, so naming a group is a single creation. A member of another Projet joins
 * this one too (ADR-0087).
 */
export function createProject(name: string, members: string[] = []): Promise<Project> {
  return request<Project>("POST", "/projects", {
    body: members.length > 0 ? { name, members } : { name },
    label: "POST /projects",
  });
}

/**
 * Rename a Projet and/or (re)set the harness it carries (`PATCH /projects/{id}`).
 * `harness`: a string sets it, `null` clears it, `undefined` leaves it unchanged
 * (double-`Option` on the wire). `name` omitted leaves it unchanged.
 */
export interface UpdateProjectRequest {
  name?: string;
  harness?: string | null;
  agent_choice?: AgentChoice | null;
  /** #669: replace the Projet's skills wholesale; `[]` clears. */
  skills?: SkillRef[];
  /** ADR-0087: the default primary repository, a member; `null` goes back to the first one. */
  primary_repo?: string | null;
}

export function updateProject(
  projectId: string,
  req: UpdateProjectRequest,
): Promise<Project> {
  return request<Project>("PATCH", `/projects/${encodeURIComponent(projectId)}`, {
    body: req,
    label: `PATCH /projects/${projectId}`,
  });
}

/**
 * Attach a member path to a Projet (`POST /projects/{id}/members`). A path may be
 * a member of several Projets (ADR-0087); attaching it twice is a no-op.
 */
export function addProjectMember(projectId: string, path: string): Promise<Project> {
  return request<Project>(
    "POST",
    `/projects/${encodeURIComponent(projectId)}/members`,
    { body: { path }, label: `POST /projects/${projectId}/members` },
  );
}

export function removeProjectMember(projectId: string, path: string): Promise<Project> {
  return request<Project>(
    "DELETE",
    `/projects/${encodeURIComponent(projectId)}/members`,
    { body: { path }, label: `DELETE /projects/${projectId}/members` },
  );
}

export function deleteProject(projectId: string): Promise<void> {
  return request<void>("DELETE", `/projects/${encodeURIComponent(projectId)}`, {
    responseMode: "void",
    label: `DELETE /projects/${projectId}`,
  });
}
