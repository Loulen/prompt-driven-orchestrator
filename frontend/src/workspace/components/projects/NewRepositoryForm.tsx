import { useState } from "react";
import { FolderOpen, FolderPlus } from "lucide-react";
import FsExplorerModal from "../FsExplorerModal";
import { tildify } from "../../../lib/homePath";
import { newRepositoryTarget } from "../../model/projectsSettings";

/**
 * « New empty repository » of a Projet in Settings › Projects (CORE-96): the
 * folder to create (or an empty one to take), then `git init` and a first
 * commit. A folder that holds something and is not a repository is refused by
 * the daemon, whose sentence shows here.
 */
export default function NewRepositoryForm({
  home,
  canBrowse,
  onCreate,
  onCancel,
}: {
  home: string | null;
  canBrowse: boolean;
  /** Creates the repository and adds it; rejects with the sentence to show. */
  onCreate: (parent: string, name: string) => Promise<void>;
  onCancel: () => void;
}) {
  // Untouched, the field starts at the home (which may arrive after the form opens).
  const [edited, setTyped] = useState<string | null>(null);
  const typed = edited ?? (home ? "~/" : "/");
  const [explorer, setExplorer] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const target = newRepositoryTarget(typed, home);

  async function create() {
    if (!target || busy) return;
    setBusy(true);
    setError(null);
    try {
      await onCreate(target.parent, target.name);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  }

  return (
    <div
      className="flex flex-col gap-2 rounded-md border border-acc-border bg-bg-2 px-3 py-2.5"
      data-testid="project-new-repo"
    >
      <div className="flex items-center gap-1.5 font-medium text-fg" style={{ fontSize: "11.5px" }}>
        <FolderPlus size={12} className="text-acc" />
        New empty repository
      </div>
      <div className="flex gap-1.5">
        <input
          autoFocus
          value={typed}
          onChange={(e) => {
            setTyped(e.target.value);
            setError(null);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") void create();
            if (e.key === "Escape") {
              e.preventDefault();
              onCancel();
            }
          }}
          aria-label="Folder of the new repository"
          placeholder="~/code/new-repository"
          data-testid="project-new-repo-path"
          className="min-w-0 flex-1 rounded border border-line-strong bg-bg-3 px-2.5 py-1.5 font-mono text-fg outline-none focus:border-acc"
          style={{ fontSize: "12px" }}
        />
        {canBrowse && (
          <button
            type="button"
            onClick={() => setExplorer(true)}
            title="Choose the parent folder"
            aria-label="Choose the parent folder"
            data-testid="project-new-repo-browse"
            className="shrink-0 rounded border border-line-strong bg-bg-3 px-2 text-fg-3 hover:border-acc hover:text-fg"
          >
            <FolderOpen size={13} />
          </button>
        )}
      </div>
      <div className="flex items-center justify-between gap-2 text-fg-4" style={{ fontSize: "10.5px" }}>
        <span className="truncate">
          {target ? (
            <>
              Into <span className="font-mono text-fg-3">{tildify(target.parent, home)}</span> ·{" "}
              <span className="font-mono text-fg-3">{target.name}</span>, created or empty
            </>
          ) : (
            "An absolute path, ending with the folder name"
          )}
        </span>
        <span className="shrink-0">git init · first commit</span>
      </div>
      {error && (
        <div className="text-st-failed" style={{ fontSize: "10.5px" }} data-testid="project-new-repo-error">
          {error}
        </div>
      )}
      <div className="flex justify-end gap-1.5">
        <button
          type="button"
          onClick={onCancel}
          className="rounded px-2.5 py-1 text-fg-3 hover:bg-bg-4"
          style={{ fontSize: "11px" }}
        >
          Cancel
        </button>
        <button
          type="button"
          onClick={() => void create()}
          disabled={!target || busy}
          data-testid="project-new-repo-create"
          className="rounded bg-acc px-2.5 py-1 font-medium text-on-acc hover:bg-acc-dim disabled:opacity-50"
          style={{ fontSize: "11px" }}
        >
          {busy ? "Creating…" : "Create and add"}
        </button>
      </div>
      {explorer && (
        <FsExplorerModal
          title="Where to create the repository"
          confirmLabel="Create it in this folder"
          startPath={target?.parent}
          onPick={(parent) => {
            const name = target?.name ?? "new-repository";
            setTyped(`${tildify(parent.replace(/\/+$/, ""), home)}/${name}`);
            setError(null);
          }}
          onClose={() => setExplorer(false)}
        />
      )}
    </div>
  );
}
