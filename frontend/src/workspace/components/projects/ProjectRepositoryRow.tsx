import { FolderGit2, X } from "lucide-react";
import { tildify } from "../../../lib/homePath";
import { Button, IconButton } from "../../../shared/ui/kit";
import MidPath from "../../../shared/ui/MidPath";

/**
 * A repository of the Projet shown in Settings › Projects (CORE-96): its path,
 * its remote, how many other Projets hold it (ADR-0087, named in the tooltip),
 * the « default » mark — or « Make default » on hover — and its removal.
 */
export default function ProjectRepositoryRow({
  path,
  home,
  remote,
  alsoIn,
  primary,
  editRefused,
  onMakePrimary,
  onRemove,
}: {
  path: string;
  home: string | null;
  /** `undefined` while it is read, `null` for a repository without a remote. */
  remote: string | null | undefined;
  alsoIn: string[];
  primary: boolean;
  /** CORE-205: why the caller may not change the Projet (its gestures greyed), else absent. */
  editRefused?: string;
  onMakePrimary: () => void;
  onRemove: () => void;
}) {
  return (
    <div className="set-rrow" data-testid="project-repo-row" data-path={path} title={remote ? `${path}\n${remote}` : path}>
      <FolderGit2 size={13} strokeWidth={1.5} aria-hidden />
      <MidPath path={tildify(path, home)} title={false} className="set-rrow__path" testId="project-repo-path" />
      {primary && (
        <span className="d-chip" title="Preselected as the target in New Run" data-testid="project-repo-primary">
          default
        </span>
      )}
      {!primary && (
        <Button
          size="sm"
          tone="ghost"
          className="set-hover"
          onClick={onMakePrimary}
          disabledReason={editRefused}
          data-testid="project-repo-make-primary"
        >
          Make default
        </Button>
      )}
      <span
        className="set-mono set-ellipsis set-rrow__remote"
        title={remote ?? "no remote"}
        data-testid="project-repo-remote"
      >
        {remote === undefined ? "…" : (remote ?? "no remote")}
      </span>
      {alsoIn.length > 0 && (
        <span className="d-chip" title={`Also in ${alsoIn.join(", ")}`} data-testid="project-repo-also-in">
          +{alsoIn.length}
        </span>
      )}
      <IconButton
        icon={X}
        label={`Remove ${path} from the project`}
        description="The folder stays on disk"
        disabledReason={editRefused}
        size="sm"
        className="set-hover"
        onClick={onRemove}
        data-testid="project-repo-remove"
      />
    </div>
  );
}
