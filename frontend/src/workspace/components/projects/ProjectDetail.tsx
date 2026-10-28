import { useState } from "react";
import type { CloneJob } from "../../../shared/generated/CloneJob";
import { cancelClone } from "../../api/repos";
import { useClones } from "../../store/useClones";
import { Pencil, SlidersHorizontal, Trash2 } from "lucide-react";
import type { Project } from "../../model/project";
import { alsoIn, recentToAdd, runCount } from "../../model/projectsSettings";
import FsExplorerModal from "../FsExplorerModal";
import AddRepositoryMenu, { type AddRepositoryChoice } from "./AddRepositoryMenu";
import CloneRepositoryForm from "./CloneRepositoryForm";
import NewRepositoryForm from "./NewRepositoryForm";
import ProjectRepositoryRow from "./ProjectRepositoryRow";

/** What the detail of a Projet asks its page to do; each rejects with the sentence to show. */
export interface ProjectGestures {
  rename: (name: string) => Promise<void>;
  addRepository: (path: string) => Promise<void>;
  /** Validates the folder picked in the explorer, then adds it. */
  addLocalFolder: (path: string) => Promise<void>;
  createRepository: (parent: string, name: string) => Promise<void>;
  /** Clones `url` (into `path` when given) for this Projet, in the daemon (CORE-97). */
  cloneRepository: (url: string, path: string | null) => Promise<CloneJob>;
  /** A clone of this Projet ended: read the Projets again. */
  refresh: () => void;
  removeRepository: (path: string) => Promise<void>;
  makePrimary: (path: string) => Promise<void>;
  /** Opens the confirmation; the page deletes. */
  askDelete: () => void;
  /** Opens the Projet's other settings (agent, skills, provisioning, extensions). */
  openSettings: () => void;
}

/**
 * The detail of a Projet in Settings › Projects (CORE-96): its name (the
 * pencil renames it), how many Runs were launched in it, its repositories and
 * its default primary, « Add repository » and « Delete project ». Read-only
 * when the caller may not update the Projet.
 */
export default function ProjectDetail({
  project,
  projects,
  runs,
  remotes,
  recent,
  home,
  editable,
  deletable,
  canBrowse,
  canCreateRepository,
  gestures,
}: {
  project: Project;
  projects: Project[];
  runs: number;
  remotes: Map<string, string | null>;
  recent: string[];
  home: string | null;
  editable: boolean;
  deletable: boolean;
  canBrowse: boolean;
  canCreateRepository: boolean;
  gestures: ProjectGestures;
}) {
  const [renaming, setRenaming] = useState<string | null>(null);
  const [adding, setAdding] = useState<"local" | "new" | "clone" | null>(null);
  const [cloneUrl, setCloneUrl] = useState("");
  const [dismissed, setDismissed] = useState<ReadonlySet<string>>(() => new Set());
  // The URL each clone started here was typed with: a clone shows it without
  // its credentials, « Try again » takes it back whole.
  const [typedUrls, setTypedUrls] = useState<ReadonlyMap<string, string>>(() => new Map());
  // The clones of this Projet, read again while one runs: one started before
  // Settings was last closed shows where it is.
  const { jobs, track } = useClones(editable, (job) => {
    if (job.state === "done" && job.project_ids.includes(project.id)) gestures.refresh();
  });
  const clones = jobs.filter(
    (job) =>
      job.project_ids.includes(project.id) &&
      (job.state === "running" || (job.state === "failed" && !dismissed.has(job.id))),
  );
  // A failed clone, once read, is dismissed for good: the daemon drops it from
  // its list, so it does not come back when Settings opens again.
  function dismiss(id: string) {
    setDismissed((prev) => new Set(prev).add(id));
    void cancelClone(id).catch(() => {});
  }
  const [error, setError] = useState<string | null>(null);

  async function attempt(gesture: () => Promise<void>): Promise<boolean> {
    setError(null);
    try {
      await gesture();
      return true;
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      return false;
    }
  }

  async function commitRename() {
    if (renaming === null) return;
    const name = renaming.trim();
    if (!name || name === project.name) {
      setRenaming(null);
      return;
    }
    if (await attempt(() => gestures.rename(name))) setRenaming(null);
  }

  function choose(choice: AddRepositoryChoice) {
    if (choice.kind === "recent") void attempt(() => gestures.addRepository(choice.path));
    else {
      if (choice.kind === "clone") setCloneUrl("");
      setAdding(choice.kind);
    }
  }

  return (
    <div
      className="flex min-w-0 flex-1 flex-col gap-3 rounded-md border border-line bg-surface-0 px-3.5 py-3"
      data-testid="project-detail"
      data-project-id={project.id}
    >
      <div className="flex items-center gap-2">
        {renaming !== null ? (
          <input
            autoFocus
            value={renaming}
            aria-label="Project name"
            data-testid="project-rename-input"
            onChange={(e) => setRenaming(e.target.value)}
            onBlur={() => void commitRename()}
            onKeyDown={(e) => {
              if (e.key === "Enter") void commitRename();
              if (e.key === "Escape") {
                e.preventDefault();
                setRenaming(null);
              }
            }}
            className="min-w-0 rounded border border-primary bg-surface-2 px-1.5 py-0.5 font-semibold text-text outline-none"
            style={{ fontSize: "13px" }}
          />
        ) : (
          <span className="truncate font-semibold text-text" style={{ fontSize: "13px" }} data-testid="project-detail-name">
            {project.name}
          </span>
        )}
        {editable && renaming === null && (
          <button
            type="button"
            onClick={() => setRenaming(project.name)}
            aria-label="Rename project"
            title="Rename"
            data-testid="project-rename"
            className="shrink-0 rounded p-0.5 text-text-3 transition-colors hover:bg-hover hover:text-text-2"
          >
            <Pencil size={11} />
          </button>
        )}
        <span className="shrink-0 font-mono text-text-3" style={{ fontSize: "10.5px" }} data-testid="project-detail-runs">
          {runCount(runs)}
        </span>
        {!editable && (
          <span
            className="shrink-0 rounded bg-surface-2 px-1.5 py-0.5 text-text-3"
            style={{ fontSize: "9.5px" }}
            title="You may read this project, not change it."
            data-testid="project-read-only"
          >
            read-only
          </span>
        )}
        <div className="ml-auto flex shrink-0 items-center gap-1.5">
          {editable && (
            <button
              type="button"
              onClick={gestures.openSettings}
              title="Agent, skills, worktree provisioning and extension settings of this project"
              data-testid="project-more-settings"
              className="flex items-center gap-1 rounded border border-line-strong px-2 py-1 text-text-3 transition-colors hover:border-primary hover:text-text"
              style={{ fontSize: "10.5px" }}
            >
              <SlidersHorizontal size={11} />
              Agent & skills…
            </button>
          )}
          {deletable && (
            <button
              type="button"
              onClick={gestures.askDelete}
              data-testid="project-delete"
              className="flex items-center gap-1 rounded border border-st-failed/50 px-2 py-1 text-st-failed transition-colors hover:bg-st-failed/10"
              style={{ fontSize: "10.5px" }}
            >
              <Trash2 size={11} />
              Delete project
            </button>
          )}
        </div>
      </div>

      <div className="flex items-baseline justify-between gap-2">
        <span className="font-medium text-text-2" style={{ fontSize: "11.5px" }}>
          Repositories
        </span>
        <span className="text-text-3" style={{ fontSize: "10.5px" }}>
          the default primary is preselected in New Run
        </span>
      </div>

      <div className="flex flex-col gap-1.5" data-testid="project-detail-repos">
        {project.members.length === 0 && (
          <div className="px-1 text-text-3" style={{ fontSize: "11px" }}>
            No repository yet.
          </div>
        )}
        {project.members.map((path) => (
          <ProjectRepositoryRow
            key={path}
            path={path}
            home={home}
            remote={remotes.get(path)}
            alsoIn={alsoIn(projects, project.id, path)}
            primary={project.primary_repo === path}
            editable={editable}
            onMakePrimary={() => void attempt(() => gestures.makePrimary(path))}
            onRemove={() => void attempt(() => gestures.removeRepository(path))}
          />
        ))}
      </div>

      {error && (
        <div className="text-st-failed" style={{ fontSize: "10.5px" }} data-testid="project-detail-error">
          {error}
        </div>
      )}

      {clones.map((job) => (
        <CloneRepositoryForm
          key={job.id}
          home={home}
          projectName={project.name}
          job={job}
          onCancel={() => dismiss(job.id)}
          onCancelClone={() => void cancelClone(job.id).then(track).catch(() => {})}
          onRetry={() => {
            dismiss(job.id);
            setCloneUrl(typedUrls.get(job.id) ?? job.url);
            setAdding("clone");
          }}
        />
      ))}

      {editable &&
        (adding === "clone" ? (
          <CloneRepositoryForm
            // A retry fills the form again.
            key={cloneUrl}
            home={home}
            projectName={project.name}
            initialUrl={cloneUrl}
            onClone={async (url, path) => {
              const job = await gestures.cloneRepository(url, path);
              setTypedUrls((prev) => new Map(prev).set(job.id, url));
              track(job);
              if (job.state === "done") gestures.refresh();
              setAdding(null);
            }}
            onCancel={() => setAdding(null)}
          />
        ) : adding === "new" ? (
          <NewRepositoryForm
            home={home}
            canBrowse={canBrowse}
            onCreate={async (parent, name) => {
              await gestures.createRepository(parent, name);
              setAdding(null);
            }}
            onCancel={() => setAdding(null)}
          />
        ) : (
          <AddRepositoryMenu
            recent={recentToAdd(recent, project)}
            home={home}
            canBrowse={canBrowse}
            canCreateRepository={canCreateRepository}
            onChoose={choose}
          />
        ))}

      {adding === "local" && (
        <FsExplorerModal
          title="Add a local repository"
          confirmLabel="Add this repository"
          // Opens beside the Projet's first repository, where its others likely are.
          startPath={project.members[0]?.replace(/\/[^/]+\/?$/, "") || undefined}
          onPick={(path) => void attempt(() => gestures.addLocalFolder(path))}
          onClose={() => setAdding(null)}
        />
      )}
    </div>
  );
}
