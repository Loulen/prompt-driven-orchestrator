import { useState } from "react";
import { Globe, LoaderCircle } from "lucide-react";
import { tildify } from "../../../lib/homePath";
import { progressLine, progressPercent } from "../../../workspace/model/clone";
import MidPath from "../../../shared/ui/MidPath";
import type { RepoClone } from "../../store/newRun/useRepoClone";

/**
 * Under a target repository given by its URL (CORE-97), one line: where it is
 * cloned (« change » edits the folder), or that it is already cloned there;
 * then the clone's progress with its Cancel, or why it failed.
 */
export default function RemoteRepoHint({ clone, home }: { clone: RepoClone; home: string | null }) {
  const { cloneTarget, clonePath, setClonePath, cloneJob, cloneError, cloning, cancelClone } = clone;
  const [editing, setEditing] = useState(false);
  const path = clonePath ?? cloneTarget?.path ?? null;

  if (cloning && cloneJob) {
    return (
      <div className="ln-hint" data-testid="remote-repo-progress">
        <LoaderCircle size={12} strokeWidth={2} className="d-spin ln-hint__ic" aria-hidden />
        <span className="ln-hint__word">Cloning</span>
        <MidPath path={tildify(cloneJob.path, home)} className="ln-hint__path" />
        <span className="ln-faint ln-num ln-hint__prog" title={progressLine(cloneJob)}>
          {progressLine(cloneJob)}
        </span>
        <span className="ln-bar ln-grow">
          <span style={{ width: `${progressPercent(cloneJob)}%` }} />
        </span>
        <button type="button" onClick={cancelClone} className="ln-link" data-testid="remote-repo-cancel">
          Cancel
        </button>
      </div>
    );
  }

  return (
    <>
      <div className="ln-hint" data-testid="remote-repo-hint">
        <Globe size={12} strokeWidth={1.5} className="ln-hint__ic" aria-hidden />
        {editing ? (
          <>
            <span className="ln-hint__word">Clone into</span>
            <input
              autoFocus
              value={clonePath ?? (path ? tildify(path, home) : "")}
              onChange={(e) => setClonePath(e.target.value)}
              onBlur={() => setEditing(false)}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === "Escape") {
                  // Escape ends the edit, not the dialog.
                  e.preventDefault();
                  e.stopPropagation();
                  setEditing(false);
                }
              }}
              aria-label="Folder to clone into"
              spellCheck={false}
              data-testid="remote-repo-path"
              className="d-input d-input--sm d-input--mono ln-grow"
            />
          </>
        ) : cloneTarget?.cloned && clonePath === null ? (
          <>
            <span className="ln-hint__word">Already cloned ·</span>
            <MidPath path={tildify(cloneTarget.path, home)} className="ln-hint__path" />
          </>
        ) : (
          <>
            <span className="ln-hint__word" title="Full clone">
              Clone into
            </span>
            {path && <MidPath path={tildify(path, home)} className="ln-hint__path" />}
            <button type="button" onClick={() => setEditing(true)} className="ln-link" data-testid="remote-repo-change">
              change
            </button>
          </>
        )}
      </div>
      {cloneError && (
        <span className="ln-err" data-testid="remote-repo-error">
          {cloneError}
        </span>
      )}
    </>
  );
}
