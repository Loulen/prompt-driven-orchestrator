import { useState } from "react";
import type { CloneJob } from "../../../shared/generated/CloneJob";
import { cancelClone } from "../../api/repos";
import { useClones } from "../../store/useClones";
import { FolderGit2, Pencil, SlidersHorizontal, Trash2 } from "lucide-react";
import { Button, EmptyState, IconButton, TextInput } from "../../../shared/ui/kit";
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
    <div className="set-pdet" data-testid="project-detail" data-project-id={project.id}>
      <div className="set-pdet__head">
        {renaming !== null ? (
          <TextInput
            autoFocus
            value={renaming}
            aria-label="Project name"
            data-testid="project-rename-input"
            className="set-pdet__rename"
            onChange={(e) => setRenaming(e.target.value)}
            onBlur={() => void commitRename()}
            onKeyDown={(e) => {
              if (e.key === "Enter") void commitRename();
              if (e.key === "Escape") {
                e.preventDefault();
                setRenaming(null);
              }
            }}
          />
        ) : (
          <span className="set-pdet__name" title={project.name} data-testid="project-detail-name">
            {project.name}
          </span>
        )}
        {editable && renaming === null && (
          <IconButton
            icon={Pencil}
            label="Rename project"
            size="sm"
            onClick={() => setRenaming(project.name)}
            data-testid="project-rename"
          />
        )}
        <span className="set-pdet__runs" data-testid="project-detail-runs">
          {runCount(runs)}
        </span>
        {!editable && (
          <span className="d-chip" title="You may read this project, not change it" data-testid="project-read-only">
            read-only
          </span>
        )}
        <span className="d-grow" />
        {editable && (
          <Button
            size="sm"
            icon={SlidersHorizontal}
            onClick={gestures.openSettings}
            title="Agent, skills, worktree provisioning and extension settings of this project"
            data-testid="project-more-settings"
          >
            Edit
          </Button>
        )}
        {deletable && (
          <IconButton
            icon={Trash2}
            label={`Delete ${project.name}`}
            tone="danger"
            onClick={gestures.askDelete}
            data-testid="project-delete"
          />
        )}
      </div>

      <div className="set-pdet__repos" data-testid="project-detail-repos">
        {project.members.length === 0 && (
          <EmptyState icon={FolderGit2} layout="row">
            No repository
          </EmptyState>
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
        <div className="set-row__note set-row__note--failed" role="alert" data-testid="project-detail-error">
          <span className="set-ellipsis" title={error}>
            {error}
          </span>
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
