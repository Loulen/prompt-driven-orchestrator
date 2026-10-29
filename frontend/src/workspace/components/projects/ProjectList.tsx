import { useState } from "react";
import { Briefcase, Plus } from "lucide-react";
import type { Project } from "../../model/project";
import { repoCount } from "../../model/projectsSettings";
import { tildify } from "../../../lib/homePath";
import { Button, TextInput } from "../../../shared/ui/kit";

/** A Projet being named in the list: a fresh one, or one a group's pencil starts. */
export interface ProjectDraft {
  name: string;
  members: string[];
}

/**
 * The column of Settings › Projects listing the Projets (CORE-96), with
 * « New project » naming one in place: Enter creates it, Escape gives up.
 */
export default function ProjectList({
  projects,
  selectedId,
  onSelect,
  draft,
  onDraft,
  onCreate,
  canCreate,
  home,
}: {
  projects: Project[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  /** The Projet being named, `null` when none is. */
  draft: ProjectDraft | null;
  onDraft: (draft: ProjectDraft | null) => void;
  onCreate: (draft: ProjectDraft) => Promise<void>;
  canCreate: boolean;
  home: string | null;
}) {
  const [creating, setCreating] = useState(false);

  async function create() {
    if (!draft || !draft.name.trim() || creating) return;
    setCreating(true);
    try {
      await onCreate({ ...draft, name: draft.name.trim() });
    } finally {
      setCreating(false);
    }
  }

  return (
    <div className="set-md__list" data-testid="projects-list">
      {projects.map((project) => {
        const selected = project.id === selectedId;
        return (
          <button
            key={project.id}
            type="button"
            onClick={() => onSelect(project.id)}
            aria-current={selected ? "true" : undefined}
            data-testid="projects-list-item"
            data-project-id={project.id}
            className="set-md__item"
            title={project.name}
          >
            <Briefcase size={13} strokeWidth={1.5} aria-hidden />
            <span className="set-ellipsis flex-1">{project.name}</span>
            <span className="set-faint set-mono text-xs" title={repoCount(project.members.length)}>
              {project.members.length}
            </span>
          </button>
        );
      })}

      {/* A pencil's draft too: no naming without the right to create. */}
      {draft && canCreate ? (
        <div className="set-md__naming">
          <div className="set-line">
            <TextInput
              autoFocus
              value={draft.name}
              placeholder="Project name"
              aria-label="New project name"
              data-testid="projects-new-name"
              disabled={creating}
              onChange={(e) => onDraft({ ...draft, name: e.target.value })}
              onKeyDown={(e) => {
                if (e.key === "Enter") void create();
                if (e.key === "Escape") {
                  // Gives up the name, not Settings (the shell skips a handled Escape).
                  e.preventDefault();
                  onDraft(null);
                }
              }}
            />
            <Button
              size="sm"
              tone="primary"
              onClick={() => void create()}
              disabled={!draft.name.trim() || creating}
              data-testid="projects-new-create"
            >
              {creating ? "Creating…" : "Create"}
            </Button>
          </div>
          {draft.members.length > 0 && (
            <span
              className="set-faint set-ellipsis text-xs"
              title={draft.members.join("\n")}
              data-testid="projects-new-members"
            >
              with {draft.members.map((path) => tildify(path, home)).join(", ")}
            </span>
          )}
        </div>
      ) : (
        canCreate && (
          <button
            type="button"
            onClick={() => onDraft({ name: "", members: [] })}
            data-testid="projects-new"
            className="set-md__item set-md__new"
          >
            <Plus size={13} strokeWidth={1.5} aria-hidden />
            <span>New project</span>
          </button>
        )
      )}
    </div>
  );
}
