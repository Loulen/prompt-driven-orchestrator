import { FolderGit2, X } from "lucide-react";
import { tildify } from "../../../lib/homePath";

/**
 * A repository of the Projet shown in Settings › Projects (CORE-96): its path,
 * its remote, the other Projets holding it (ADR-0087), the « default primary »
 * mark — or « Make default » on hover — and its removal.
 */
export default function ProjectRepositoryRow({
  path,
  home,
  remote,
  alsoIn,
  primary,
  editable,
  onMakePrimary,
  onRemove,
}: {
  path: string;
  home: string | null;
  /** `undefined` while it is read, `null` for a repository without a remote. */
  remote: string | null | undefined;
  alsoIn: string[];
  primary: boolean;
  editable: boolean;
  onMakePrimary: () => void;
  onRemove: () => void;
}) {
  return (
    <div
      className="group/repo flex items-center gap-2.5 rounded-md border border-line-soft bg-bg-3/50 px-2.5 py-2"
      data-testid="project-repo-row"
      data-path={path}
      title={path}
    >
      <FolderGit2 size={13} className="shrink-0 text-fg-4" />
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <div className="flex min-w-0 items-center gap-2">
          <span className="truncate font-mono text-fg" style={{ fontSize: "11.5px" }}>
            {tildify(path, home)}
          </span>
          {primary && (
            <span
              className="shrink-0 rounded bg-acc/15 px-1.5 py-0.5 text-acc"
              style={{ fontSize: "9.5px" }}
              data-testid="project-repo-primary"
            >
              default primary
            </span>
          )}
          {!primary && editable && (
            <button
              type="button"
              onClick={onMakePrimary}
              className="hidden shrink-0 rounded px-1.5 py-0.5 text-fg-4 transition-colors hover:bg-bg-4 hover:text-fg-2 group-hover/repo:inline-flex group-focus-within/repo:inline-flex"
              style={{ fontSize: "10px" }}
              data-testid="project-repo-make-primary"
            >
              Make default
            </button>
          )}
        </div>
        <div className="flex min-w-0 items-center gap-1 text-fg-4" style={{ fontSize: "10.5px" }}>
          <span className="truncate font-mono" data-testid="project-repo-remote">
            {remote === undefined ? "…" : (remote ?? "no remote")}
          </span>
          {alsoIn.length > 0 && (
            <span className="shrink-0" data-testid="project-repo-also-in">
              · also in {alsoIn.join(", ")}
            </span>
          )}
        </div>
      </div>
      {editable && (
        <button
          type="button"
          onClick={onRemove}
          aria-label={`Remove ${path} from the project`}
          title="Remove from the project (the folder stays on disk)"
          className="shrink-0 rounded p-1 text-fg-4 transition-colors hover:bg-bg-4 hover:text-fg-2"
          data-testid="project-repo-remove"
        >
          <X size={12} />
        </button>
      )}
    </div>
  );
}
