import { useState } from "react";
import { Briefcase, Plus } from "lucide-react";
import type { Project } from "../../model/project";
import { repoCount } from "../../model/projectsSettings";
import { tildify } from "../../../lib/homePath";

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
    <div className="flex w-[220px] shrink-0 flex-col gap-0.5" data-testid="projects-list">
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
            className={`flex items-center gap-2 rounded-md px-2.5 py-2 text-left transition-colors ${
              selected ? "bg-bg-3 text-fg" : "text-fg-2 hover:bg-bg-3/60"
            }`}
            style={{ fontSize: "11.5px" }}
          >
            <Briefcase size={12} className="shrink-0 text-fg-4" />
            <span className={`min-w-0 flex-1 truncate ${selected ? "font-semibold" : ""}`}>
              {project.name}
            </span>
            <span className="shrink-0 font-mono text-fg-4" style={{ fontSize: "10px" }}>
              {repoCount(project.members.length)}
            </span>
          </button>
        );
      })}

      {/* A pencil's draft too: no naming without the right to create. */}
      {draft && canCreate ? (
        <div className="mt-1 flex flex-col gap-1 rounded-md border border-acc-border bg-bg-3 px-2 py-1.5">
          <input
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
            className="w-full bg-transparent text-fg outline-none placeholder:text-fg-4"
            style={{ fontSize: "11.5px" }}
          />
          {draft.members.length > 0 && (
            <span className="truncate text-fg-4" style={{ fontSize: "10px" }} data-testid="projects-new-members">
              with {draft.members.map((path) => tildify(path, home)).join(", ")}
            </span>
          )}
          <div className="flex justify-end gap-1.5">
            <button
              type="button"
              onClick={() => onDraft(null)}
              className="rounded px-2 py-0.5 text-fg-3 hover:bg-bg-4"
              style={{ fontSize: "10.5px" }}
            >
              Cancel
            </button>
            <button
              type="button"
              onClick={() => void create()}
              disabled={!draft.name.trim() || creating}
              data-testid="projects-new-create"
              className="rounded bg-acc px-2 py-0.5 font-medium text-on-acc hover:bg-acc-dim disabled:opacity-50"
              style={{ fontSize: "10.5px" }}
            >
              {creating ? "Creating…" : "Create"}
            </button>
          </div>
        </div>
      ) : (
        canCreate && (
          <button
            type="button"
            onClick={() => onDraft({ name: "", members: [] })}
            data-testid="projects-new"
            className="mt-1 flex items-center gap-1.5 self-start rounded-md border border-line px-2.5 py-1.5 text-fg-3 transition-colors hover:border-acc hover:text-acc"
            style={{ fontSize: "11px" }}
          >
            <Plus size={11} />
            New project
          </button>
        )
      )}
    </div>
  );
}
