import { useState } from "react";
import { tildify } from "../../../lib/homePath";
import { progressLine, progressPercent } from "../../../workspace/model/clone";
import type { RepoClone } from "../../store/newRun/useRepoClone";

/**
 * Under a target repository given by its URL (CORE-97): where it will be cloned
 * (« change » edits the folder), whether it is already cloned there, then the
 * clone's progress with its Cancel, or why it failed.
 */
export default function RemoteRepoHint({ clone, home }: { clone: RepoClone; home: string | null }) {
  const { cloneTarget, clonePath, setClonePath, cloneJob, cloneError, cloning, cancelClone } = clone;
  const [editing, setEditing] = useState(false);
  const path = clonePath ?? cloneTarget?.path ?? null;

  if (cloning && cloneJob) {
    return (
      <div className="mt-1 flex flex-col gap-1" data-testid="remote-repo-progress">
        <div className="flex items-center justify-between gap-2" style={{ fontSize: "10.5px" }}>
          <span className="text-st-running">
            Cloning into <span className="font-mono">{tildify(cloneJob.path, home)}</span> ·{" "}
            {progressLine(cloneJob)}
          </span>
          <button
            type="button"
            onClick={cancelClone}
            className="text-text-3 hover:text-text-2"
            data-testid="remote-repo-cancel"
          >
            Cancel
          </button>
        </div>
        <div className="h-1 overflow-hidden rounded-full bg-surface-2">
          <div
            className="h-full rounded-full bg-st-running transition-[width] duration-300"
            style={{ width: `${progressPercent(cloneJob)}%` }}
          />
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-0.5" style={{ fontSize: "10.5px" }}>
      <span className="flex flex-wrap items-center gap-1 text-st-running" data-testid="remote-repo-hint">
        {editing ? (
          <>
            Remote URL · will be cloned in full to
            <input
              autoFocus
              value={clonePath ?? (path ? tildify(path, home) : "")}
              onChange={(e) => setClonePath(e.target.value)}
              onBlur={() => setEditing(false)}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === "Escape") setEditing(false);
              }}
              aria-label="Folder to clone into"
              spellCheck={false}
              data-testid="remote-repo-path"
              className="min-w-0 flex-1 rounded border border-line-strong bg-surface-2 px-1.5 py-0.5 font-mono text-text outline-none focus:border-st-running"
            />
          </>
        ) : cloneTarget?.cloned && clonePath === null ? (
          // One flex item: the row's gap would part the path from its comma.
          <span>
            Remote URL · already cloned in{" "}
            <span className="font-mono">{tildify(cloneTarget.path, home)}</span>, reused
          </span>
        ) : (
          <>
            Remote URL · will be cloned in full
            {path && (
              <>
                {" "}
                to <span className="font-mono">{tildify(path, home)}</span>
              </>
            )}
            <button
              type="button"
              onClick={() => setEditing(true)}
              className="underline"
              data-testid="remote-repo-change"
            >
              change
            </button>
          </>
        )}
      </span>
      {cloneError && (
        <span className="text-st-failed" data-testid="remote-repo-error">
          {cloneError}
        </span>
      )}
    </div>
  );
}
