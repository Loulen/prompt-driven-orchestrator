import { useEffect, useState } from "react";
import { Globe, Loader2 } from "lucide-react";
import { tildify } from "../../../lib/homePath";
import { fetchCloneTarget } from "../../api/repos";
import { isRemoteUrl, progressLine, progressPercent } from "../../model/clone";
import type { CloneJob } from "../../../shared/generated/CloneJob";
import type { CloneTarget } from "../../../shared/generated/CloneTarget";

/**
 * « Clone from a URL » of a Projet in Settings › Projects (CORE-97): the URL, the
 * folder it is cloned into (`<clones folder>/<name>`, which « change » edits), then
 * the clone's progress with its Cancel. The clone runs in the daemon: Settings may
 * close, the repository joins the Projet once it is there.
 *
 * Without `job`, the form to fill; with one, that clone as it runs (or why it failed).
 */
export default function CloneRepositoryForm({
  home,
  projectName,
  job,
  initialUrl = "",
  onClone,
  onCancel,
  onCancelClone,
  onRetry,
}: {
  home: string | null;
  projectName: string;
  job?: CloneJob;
  initialUrl?: string;
  /** Starts the clone (into `path` when the folder was changed); rejects with the sentence to show. */
  onClone?: (url: string, path: string | null) => Promise<void>;
  /** Closes the form, or dismisses a failed clone. */
  onCancel: () => void;
  /** Stops the running clone. */
  onCancelClone?: () => void;
  /** A failed clone, filled again to try once more. */
  onRetry?: () => void;
}) {
  const [url, setUrl] = useState(initialUrl);
  const [target, setTarget] = useState<CloneTarget | null>(null);
  const [editingPath, setEditingPath] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const typed = url.trim();
  const valid = isRemoteUrl(typed);

  // Where the clone goes, asked again as the URL settles.
  useEffect(() => {
    if (job || !valid) return;
    let current = true;
    const timer = setTimeout(() => {
      fetchCloneTarget(typed)
        .then((answer) => current && setTarget(answer))
        .catch(() => current && setTarget(null));
    }, 250);
    return () => {
      current = false;
      clearTimeout(timer);
    };
  }, [job, valid, typed]);

  const shownTarget = valid ? target : null;
  const path = job?.path ?? editingPath ?? shownTarget?.path ?? null;
  const running = job?.state === "running";
  const failed = job?.state === "failed";

  async function clone() {
    if (!valid || busy || !onClone) return;
    setBusy(true);
    setError(null);
    try {
      await onClone(typed, editingPath?.trim() || null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  }

  return (
    <div
      className="flex flex-col gap-2 rounded-md border border-st-running/50 bg-surface-1 px-3 py-2.5"
      data-testid={job ? "project-clone-job" : "project-clone"}
      data-state={job?.state}
    >
      <div className="flex items-center gap-1.5 font-medium text-text" style={{ fontSize: "11.5px" }}>
        <Globe size={12} className="text-st-running" />
        Clone from a URL
      </div>
      {job ? (
        <div
          className="truncate rounded border border-line-strong bg-surface-2 px-2.5 py-1.5 font-mono text-text"
          style={{ fontSize: "12px" }}
          title={job.url}
          data-testid="project-clone-job-url"
        >
          {job.url}
        </div>
      ) : (
        <input
          autoFocus
          value={url}
          onChange={(e) => {
            setUrl(e.target.value);
            setError(null);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") void clone();
            if (e.key === "Escape") {
              e.preventDefault();
              onCancel();
            }
          }}
          aria-label="URL of the repository to clone"
          placeholder="https://github.com/acme/app.git"
          spellCheck={false}
          data-testid="project-clone-url"
          className="min-w-0 rounded border border-line-strong bg-surface-2 px-2.5 py-1.5 font-mono text-text outline-none focus:border-st-running"
          style={{ fontSize: "12px" }}
        />
      )}
      <div className="flex items-center justify-between gap-2 text-text-3" style={{ fontSize: "10.5px" }}>
        <span className="flex min-w-0 items-center gap-1.5 truncate">
          {editingPath !== null && !job ? (
            <>
              Into
              <input
                autoFocus
                value={editingPath}
                onChange={(e) => setEditingPath(e.target.value)}
                aria-label="Folder to clone into"
                spellCheck={false}
                data-testid="project-clone-path"
                className="min-w-0 flex-1 rounded border border-line-strong bg-surface-2 px-1.5 py-0.5 font-mono text-text outline-none focus:border-st-running"
                style={{ fontSize: "11px" }}
              />
            </>
          ) : path ? (
            <>
              Into{" "}
              <span className="font-mono text-text-3" data-testid="project-clone-into">
                {tildify(path, home)}
              </span>
              {!job && (
                <button
                  type="button"
                  onClick={() => setEditingPath(tildify(path, home))}
                  className="underline decoration-dotted hover:text-text-2"
                  data-testid="project-clone-change"
                >
                  change
                </button>
              )}
              {!job && shownTarget?.cloned && editingPath === null && (
                <span className="text-primary" data-testid="project-clone-reused">
                  · already cloned, reused
                </span>
              )}
            </>
          ) : (
            "An https://, ssh:// or git@host:path URL"
          )}
        </span>
        <span className="shrink-0">full clone · with the daemon&apos;s git credentials</span>
      </div>

      {running && job && (
        <>
          <div className="flex items-center justify-between gap-2" style={{ fontSize: "10.5px" }}>
            <span className="flex items-center gap-1.5 text-text-2" data-testid="project-clone-progress">
              <Loader2 size={11} className="animate-spin text-st-running" />
              {progressLine(job)}
            </span>
            {onCancelClone && (
              <button
                type="button"
                onClick={onCancelClone}
                className="text-text-3 hover:text-text-2"
                data-testid="project-clone-cancel"
              >
                Cancel
              </button>
            )}
          </div>
          <div className="h-1 overflow-hidden rounded-full bg-surface-2">
            <div
              className="h-full rounded-full bg-st-running transition-[width] duration-300"
              style={{ width: `${progressPercent(job)}%` }}
            />
          </div>
          <div className="text-text-3" style={{ fontSize: "10.5px" }}>
            You can close Settings: the clone goes on, and the repository joins {projectName} when it
            is done.
          </div>
        </>
      )}

      {(error || (failed && job?.error)) && (
        <div className="text-st-failed" style={{ fontSize: "10.5px" }} data-testid="project-clone-error">
          {error ?? job?.error}
        </div>
      )}

      {!running && (
        <div className="flex justify-end gap-1.5">
          <button
            type="button"
            onClick={onCancel}
            className="rounded px-2.5 py-1 text-text-3 hover:bg-hover"
            style={{ fontSize: "11px" }}
            data-testid="project-clone-dismiss"
          >
            {failed ? "Dismiss" : "Cancel"}
          </button>
          {failed && job && onRetry ? (
            <button
              type="button"
              onClick={onRetry}
              data-testid="project-clone-retry"
              className="rounded bg-primary px-2.5 py-1 font-medium text-primary-ink hover:bg-primary-hi"
              style={{ fontSize: "11px" }}
            >
              Try again
            </button>
          ) : (
            !job && (
              <button
                type="button"
                onClick={() => void clone()}
                disabled={!valid || busy}
                data-testid="project-clone-start"
                className="rounded bg-primary px-2.5 py-1 font-medium text-primary-ink hover:bg-primary-hi disabled:opacity-50"
                style={{ fontSize: "11px" }}
              >
                {busy ? "Starting…" : shownTarget?.cloned && editingPath === null ? "Add" : "Clone and add"}
              </button>
            )
          )}
        </div>
      )}
    </div>
  );
}
