import { useState } from "react";
import { FolderOpen, FolderPlus, X } from "lucide-react";
import { Button, IconButton, TextInput } from "../../../shared/ui/kit";
import FsExplorerModal from "../FsExplorerModal";
import { tildify } from "../../../lib/homePath";
import { clonesFolderTarget, newRepositoryTarget } from "../../model/projectsSettings";

/**
 * « New empty repository » of a Projet in Settings › Projects (CORE-96): the
 * folder to create (or an empty one to take), then `git init` and a first
 * commit. A folder that holds something and is not a repository is refused by
 * the daemon, whose sentence shows here.
 *
 * Who may not read the disk names the repository only: the daemon makes it in
 * its clones folder (CORE-97).
 */
export default function NewRepositoryForm({
  home,
  browseRefused,
  onCreate,
  onCancel,
}: {
  home: string | null;
  /** CORE-205: why the caller may not read the disk (a name only, in the clones folder), else absent. */
  browseRefused?: string;
  /** Creates the repository (`parent` `null`: in the clones folder) and adds it; rejects with the sentence to show. */
  onCreate: (parent: string | null, name: string) => Promise<void>;
  onCancel: () => void;
}) {
  const named = !!browseRefused;
  // Untouched, the field starts at the home (which may arrive after the form opens).
  const [edited, setTyped] = useState<string | null>(null);
  const typed = edited ?? (named ? "" : home ? "~/" : "/");
  const [explorer, setExplorer] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const target = named ? clonesFolderTarget(typed) : newRepositoryTarget(typed, home);

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
    <div className="set-cform set-cform--new" data-testid="project-new-repo">
      <div className="set-line">
        <FolderPlus size={13} strokeWidth={1.5} className="set-faint" aria-hidden />
        <TextInput
          autoFocus
          mono
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
          aria-label={named ? "Name of the new repository" : "Folder of the new repository"}
          placeholder={named ? "new-repository" : "~/code/new-repository"}
          title="Created, or an empty folder; git init and a first commit"
          data-testid="project-new-repo-path"
        />
        {!named && (
          <IconButton
            icon={FolderOpen}
            label="Choose the parent folder"
            size="sm"
            onClick={() => setExplorer(true)}
            data-testid="project-new-repo-browse"
          />
        )}
        <Button
          size="sm"
          tone="primary"
          onClick={() => void create()}
          disabled={!target || busy}
          title="git init, first commit"
          data-testid="project-new-repo-create"
        >
          {busy ? "Creating…" : "Create and add"}
        </Button>
        <IconButton icon={X} label="Cancel" kbd="Esc" size="sm" onClick={onCancel} />
      </div>
      <div className="set-line set-cform__meta">
        {target ? (
          <>
            <span className="set-faint">Into</span>
            {target.parent === null ? (
              <span className="set-faint">the clones folder</span>
            ) : (
              <span className="set-mono set-ellipsis" title={target.parent}>
                {tildify(target.parent, home)}
              </span>
            )}
            <span className="set-faint">·</span>
            <span className="set-mono">{target.name}</span>
          </>
        ) : (
          <span className="set-faint">
            {named ? "A folder name, made in the clones folder" : "An absolute path, ending with the folder name"}
          </span>
        )}
      </div>
      {error && (
        <span className="set-cform__err" title={error} data-testid="project-new-repo-error">
          {error}
        </span>
      )}
      {explorer && (
        <FsExplorerModal
          title="Where to create the repository"
          confirmLabel="Create it in this folder"
          startPath={target?.parent ?? undefined}
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
