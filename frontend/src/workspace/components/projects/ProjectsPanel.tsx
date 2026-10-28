import { useEffect, useMemo, useState } from "react";
import type { Project } from "../../model/project";
import {
  addProjectMember,
  createProject,
  deleteProject,
  removeProjectMember,
  updateProject,
} from "../../api/projects";
import { createRepo, startClone, validateRepo } from "../../api/repos";
import { runCountByProject, sortedProjects, type ProjectsEntry } from "../../model/projectsSettings";
import { useProjectRights } from "../../store/useProjectRights";
import { useRepoRemotes } from "../../store/useRepoRemotes";
import { useRecentReposStore } from "../../../stores/recentReposStore";
import { Briefcase, Plus } from "lucide-react";
import { Button, ConfirmDialog, EmptyState } from "../../../shared/ui/kit";
import ProjectEditModal from "../ProjectEditModal";
import ProjectDetail, { type ProjectGestures } from "./ProjectDetail";
import ProjectList, { type ProjectDraft } from "./ProjectList";

/**
 * Settings › Projects (CORE-96, ADR-0087): list, create, rename and delete the
 * Projets, and hold their repositories and their default primary. Every
 * gesture is written at once (« saves as you go »); the list follows the
 * Projets the shell keeps live, so a change made in another browser shows here.
 *
 * A deleted Projet's Runs keep it: the Runs list shows them under a deleted
 * Projet.
 */
export default function ProjectsPanel({
  projects,
  runs,
  home,
  entry,
  active,
  onChanged,
}: {
  projects: Project[];
  runs: ReadonlyArray<{ project_id?: string | null; effective_repo?: string | null }>;
  home: string | null;
  /** Where the page opens: a Projet, or a new one to name. */
  entry?: ProjectsEntry;
  /** The page is on screen: the rights and the recent repositories are read then. */
  active: boolean;
  /** A gesture changed a Projet: read the Projets again. */
  onChanged: () => void;
}) {
  const list = useMemo(() => sortedProjects(projects), [projects]);
  const [selectedId, setSelectedId] = useState<string | null>(
    entry && "projectId" in entry ? entry.projectId : null,
  );
  const [draft, setDraft] = useState<ProjectDraft | null>(
    entry && "draft" in entry ? entry.draft : null,
  );
  const [confirmDelete, setConfirmDelete] = useState<Project | null>(null);
  const [editing, setEditing] = useState<Project | null>(null);
  const [error, setError] = useState<string | null>(null);

  // The Projet shown: the one chosen while it exists, else the first one.
  const selected = list.find((p) => p.id === selectedId) ?? list[0] ?? null;
  const rights = useProjectRights(projects, active);
  const remotes = useRepoRemotes(selected?.members ?? []);
  const counts = useMemo(() => runCountByProject(runs), [runs]);
  const recent = useRecentReposStore((s) => s.recentRepos);
  const refreshRecent = useRecentReposStore((s) => s.refresh);
  useEffect(() => {
    if (active) void refreshRecent();
  }, [active, refreshRecent]);

  async function create(named: ProjectDraft) {
    setError(null);
    try {
      const project = await createProject(named.name, named.members);
      setDraft(null);
      setSelectedId(project.id);
      onChanged();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function changed<T>(write: Promise<T>): Promise<void> {
    await write;
    onChanged();
  }

  const gestures: ProjectGestures | null = selected && {
    rename: (name) => changed(updateProject(selected.id, { name })),
    addRepository: (path) => changed(addProjectMember(selected.id, path)),
    addLocalFolder: async (path) => {
      const check = await validateRepo(path);
      if (!check.valid) throw new Error(check.error ?? `${path} is not a git repository`);
      await changed(addProjectMember(selected.id, path));
    },
    createRepository: async (parent, name) => {
      const repo = await createRepo(parent, name);
      await changed(addProjectMember(selected.id, repo.path));
    },
    cloneRepository: (url, path) =>
      startClone({ url, project_id: selected.id, ...(path ? { path } : {}) }),
    refresh: onChanged,
    removeRepository: (path) => changed(removeProjectMember(selected.id, path)),
    makePrimary: (path) => changed(updateProject(selected.id, { primary_repo: path })),
    askDelete: () => setConfirmDelete(selected),
    openSettings: () => setEditing(selected),
  };

  async function remove(project: Project) {
    setConfirmDelete(null);
    setError(null);
    try {
      await deleteProject(project.id);
      setSelectedId(null);
      onChanged();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  return (
    <div data-testid="projects-panel">
      {error && (
        <div className="set-row__note set-row__note--failed" role="alert" data-testid="projects-error">
          <span className="set-ellipsis" title={error}>
            {error}
          </span>
        </div>
      )}
      {list.length === 0 && !draft ? (
        <EmptyState
          icon={Briefcase}
          action={
            rights.canCreate && (
              <Button size="sm" icon={Plus} onClick={() => setDraft({ name: "", members: [] })} data-testid="projects-new">
                New project
              </Button>
            )
          }
        >
          <span data-testid="projects-empty">No project yet</span>
        </EmptyState>
      ) : (
        <div className="set-md">
          <ProjectList
            projects={list}
            selectedId={selected?.id ?? null}
            onSelect={(id) => {
              setSelectedId(id);
              setError(null);
            }}
            draft={draft}
            onDraft={(next) => {
              setDraft(next);
              setError(null);
            }}
            onCreate={create}
            canCreate={rights.canCreate}
            home={home}
          />
          {selected && gestures && (
            <ProjectDetail
              // A fresh detail per Projet: no rename or form survives a switch.
              key={selected.id}
              project={selected}
              projects={projects}
              runs={counts.get(selected.id) ?? 0}
              remotes={remotes}
              recent={recent}
              home={home}
              editable={!rights.readOnly.has(selected.id)}
              deletable={!rights.undeletable.has(selected.id)}
              canBrowse={rights.canBrowse}
              canCreateRepository={rights.canCreateRepository}
              gestures={gestures}
            />
          )}
        </div>
      )}

      {confirmDelete && (
        <ConfirmDialog
          title={`Delete project ${confirmDelete.name}?`}
          consequence="Runs keep it, marked deleted"
          target={`${confirmDelete.members.length} repositor${confirmDelete.members.length === 1 ? "y stays" : "ies stay"} on disk`}
          actionLabel="Delete"
          danger
          onConfirm={() => void remove(confirmDelete)}
          onClose={() => setConfirmDelete(null)}
        />
      )}

      {editing && (
        <ProjectEditModal
          initialProject={editing}
          initialName={editing.name}
          initialMemberPaths={editing.members}
          availableRepos={candidates(editing, recent, runs)}
          projects={projects}
          onClose={() => setEditing(null)}
          onSaved={onChanged}
        />
      )}
    </div>
  );
}

/** The repositories the other settings' member list offers: the Projet's, the recent ones, the Runs'. */
function candidates(
  project: Project,
  recent: string[],
  runs: ReadonlyArray<{ effective_repo?: string | null }>,
): string[] {
  const set = new Set([...project.members, ...recent]);
  for (const run of runs) if (run.effective_repo) set.add(run.effective_repo);
  return [...set];
}
