import { useEffect, useState } from "react";
import { CircleX, Globe, LoaderCircle, RotateCcw, X } from "lucide-react";
import { Button, IconButton, TextInput } from "../../../shared/ui/kit";
import MidPath from "../../../shared/ui/MidPath";
import { tildify } from "../../../lib/homePath";
import { fetchCloneTarget } from "../../api/repos";
import { isRemoteUrl, progressLine, progressPercent } from "../../model/clone";
import type { CloneJob } from "../../../shared/generated/CloneJob";
import type { CloneTarget } from "../../../shared/generated/CloneTarget";

/**
 * « Clone from a URL » of a Projet in Settings › Projects (CORE-97): the URL, the
 * folder it is cloned into (`<clones folder>/<name>`, which « change » edits), then
 * the clone as a block: its progress line and bar, its Cancel. The clone runs in
 * the daemon: Settings may close, the repository joins the Projet once it is there.
 *
 * Without `job`, the inline form; with one, that clone as it runs (or why it
 * failed, with Dismiss and Try again).
 */
export default function CloneRepositoryForm({
  home,
  projectId,
  projectName,
  pathRefused,
  job,
  initialUrl = "",
  onClone,
  onCancel,
  onCancelClone,
  onRetry,
}: {
  home: string | null;
  /** The Projet the clone is for: where it is judged. */
  projectId?: string;
  projectName: string;
  /** CORE-205: why the caller may not clone into a folder of its choosing (« change » greyed), else absent. */
  pathRefused?: string;
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
      fetchCloneTarget(typed, projectId)
        .then((answer) => current && setTarget(answer))
        .catch(() => current && setTarget(null));
    }, 250);
    return () => {
      current = false;
      clearTimeout(timer);
    };
  }, [job, valid, typed, projectId]);

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
      className="ln-cform"
      data-testid={job ? "project-clone-job" : "project-clone"}
      data-state={job?.state ?? "form"}
    >
      <div className="ln-line">
        {failed ? (
          <CircleX size={13} strokeWidth={1.5} className="ln-cform__bad" aria-label="failed" />
        ) : running ? (
          <LoaderCircle size={13} strokeWidth={1.5} className="d-spin ln-cform__run" aria-label="cloning" />
        ) : (
          <Globe size={13} strokeWidth={1.5} className="ln-cform__run" aria-hidden />
        )}
        {job ? (
          <span className="ln-mono ln-ellipsis ln-cform__url" title={job.url} data-testid="project-clone-job-url">
            {job.url}
          </span>
        ) : (
          <TextInput
            autoFocus
            mono
            className="ln-grow"
            value={url}
            onChange={(e) => {
              setUrl(e.target.value);
              setError(null);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") void clone();
              if (e.key === "Escape") {
                e.preventDefault();
                e.stopPropagation();
                onCancel();
              }
            }}
            aria-label="URL of the repository to clone"
            placeholder="https://github.com/acme/app.git"
            spellCheck={false}
            data-testid="project-clone-url"
          />
        )}
        {running && job && (
          <span className="ln-faint ln-num ln-cform__prog" data-testid="project-clone-progress" title={progressLine(job)}>
            {progressLine(job)}
          </span>
        )}
        {job && <span className="ln-grow" />}
        {running && onCancelClone && (
          <Button size="sm" tone="ghost" onClick={onCancelClone} data-testid="project-clone-cancel">
            Cancel
          </Button>
        )}
        {failed && (
          <>
            <Button size="sm" onClick={onCancel} data-testid="project-clone-dismiss">
              Dismiss
            </Button>
            {onRetry && (
              <Button size="sm" icon={RotateCcw} onClick={onRetry} data-testid="project-clone-retry">
                Try again
              </Button>
            )}
          </>
        )}
        {!job && (
          <>
            <Button
              size="sm"
              tone="primary"
              onClick={() => void clone()}
              disabled={!valid || busy}
              data-testid="project-clone-start"
            >
              {busy ? "Starting…" : shownTarget?.cloned && editingPath === null ? "Add" : "Clone and add"}
            </Button>
            <IconButton icon={X} size="sm" label="Cancel" kbd="Esc" onClick={onCancel} data-testid="project-clone-dismiss" />
          </>
        )}
      </div>

      {running && job ? (
        <div className="ln-line ln-cform__meta">
          <span className="ln-bar ln-grow">
            <span style={{ width: `${progressPercent(job)}%` }} />
          </span>
          <span className="ln-faint ln-cform__into" title={`Joins ${projectName} when the clone is done`}>
            → <MidPath path={tildify(job.path, home)} className="ln-cform__path" testId="project-clone-into" />
          </span>
        </div>
      ) : !failed ? (
        <div className="ln-line ln-cform__meta">
          {editingPath !== null && !job ? (
            <span className="ln-cform__into">
              <span className="ln-faint">Into</span>
              <TextInput
                autoFocus
                small
                mono
                className="ln-grow"
                value={editingPath}
                onChange={(e) => setEditingPath(e.target.value)}
                aria-label="Folder to clone into"
                spellCheck={false}
                data-testid="project-clone-path"
              />
            </span>
          ) : path ? (
            <span className="ln-cform__into">
              <span className="ln-faint">Into</span>
              <MidPath path={tildify(path, home)} className="ln-cform__path" testId="project-clone-into" />
              {!job && (
                <button
                  type="button"
                  onClick={() => setEditingPath(tildify(path, home))}
                  className="ln-link"
                  disabled={!!pathRefused}
                  title={pathRefused}
                  data-testid="project-clone-change"
                >
                  change
                </button>
              )}
              {!job && shownTarget?.cloned && editingPath === null && (
                <span className="d-chip" title="Already cloned there: reused once the remote accepts your git credentials" data-testid="project-clone-reused">
                  reused
                </span>
              )}
            </span>
          ) : (
            <span className="ln-faint">https://, ssh:// or git@host:path</span>
          )}
          <span className="ln-grow" />
          <span className="ln-faint" title="With the git credentials an extension provides for you, otherwise the daemon's">
            full clone
          </span>
        </div>
      ) : null}

      {(error || (failed && job?.error)) && (
        <div className="ln-err ln-cform__err" title={error ?? job?.error ?? undefined} data-testid="project-clone-error">
          {error ?? job?.error}
        </div>
      )}
    </div>
  );
}
