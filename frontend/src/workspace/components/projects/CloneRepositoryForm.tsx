import { useEffect, useState } from "react";
import { CircleX, Globe, Loader2, RotateCcw, X } from "lucide-react";
import { tildify } from "../../../lib/homePath";
import { Button, IconButton, TextInput } from "../../../shared/ui/kit";
import { fetchCloneTarget } from "../../api/repos";
import { isRemoteUrl, progressLine, progressPercent } from "../../model/clone";
import type { CloneJob } from "../../../shared/generated/CloneJob";
import type { CloneTarget } from "../../../shared/generated/CloneTarget";

/**
 * « Clone from a URL » of a Projet in Settings › Projects (CORE-97), as a block: the
 * URL, the folder it is cloned into (`<clones folder>/<name>`, which « change » edits),
 * then the clone's progress line and bar with its Cancel, or why it failed with Dismiss
 * and Try again. The clone runs in the daemon: Settings may close, the repository joins
 * the Projet once it is there.
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

  const into = path && (
    <span className="set-line min-w-0">
      <span className="set-faint">Into</span>
      <span className="set-mono set-ellipsis" title={path} data-testid="project-clone-into">
        {tildify(path, home)}
      </span>
    </span>
  );

  if (job) {
    return (
      <div className="set-cform" data-testid="project-clone-job" data-state={job.state}>
        <div className="set-line">
          {failed ? (
            <CircleX size={13} strokeWidth={1.5} className="set-bad" aria-label="failed" />
          ) : (
            <Loader2 size={13} strokeWidth={1.5} className="animate-spin text-st-running" aria-label="cloning" />
          )}
          <span className="set-mono set-ellipsis" title={job.url} data-testid="project-clone-job-url">
            {job.url}
          </span>
          {running && (
            <span className="set-faint set-mono flex-none text-xs" data-testid="project-clone-progress">
              {progressLine(job)}
            </span>
          )}
          <span className="d-grow" />
          {running && onCancelClone && (
            <Button size="sm" tone="ghost" onClick={onCancelClone} data-testid="project-clone-cancel">
              Cancel
            </Button>
          )}
          {!running && (
            <Button size="sm" onClick={onCancel} data-testid="project-clone-dismiss">
              Dismiss
            </Button>
          )}
          {failed && onRetry && (
            <Button size="sm" icon={RotateCcw} onClick={onRetry} data-testid="project-clone-retry">
              Try again
            </Button>
          )}
        </div>
        {running ? (
          <div className="set-line set-cform__meta">
            <span className="set-bar">
              <span style={{ width: `${progressPercent(job)}%` }} />
            </span>
            <span className="set-line min-w-0" title={`The repository joins ${projectName} when the clone is done`}>
              → {into}
            </span>
          </div>
        ) : (
          job.error && (
            <span className="set-cform__err" title={job.error} data-testid="project-clone-error">
              {job.error}
            </span>
          )
        )}
      </div>
    );
  }

  return (
    <div className="set-cform" data-testid="project-clone">
      <div className="set-line">
        <Globe size={13} strokeWidth={1.5} className="text-st-running" aria-hidden />
        <TextInput
          autoFocus
          mono
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
          title="A full clone, with the daemon's git credentials"
          spellCheck={false}
          data-testid="project-clone-url"
        />
        <Button
          size="sm"
          tone="primary"
          onClick={() => void clone()}
          disabled={!valid || busy}
          data-testid="project-clone-start"
        >
          {busy ? "Starting…" : shownTarget?.cloned && editingPath === null ? "Add" : "Clone and add"}
        </Button>
        <IconButton icon={X} label="Cancel" kbd="Esc" size="sm" onClick={onCancel} data-testid="project-clone-dismiss" />
      </div>
      <div className="set-line set-cform__meta">
        {editingPath !== null ? (
          <>
            <span className="set-faint">Into</span>
            <TextInput
              autoFocus
              small
              mono
              value={editingPath}
              onChange={(e) => setEditingPath(e.target.value)}
              aria-label="Folder to clone into"
              spellCheck={false}
              data-testid="project-clone-path"
            />
          </>
        ) : path ? (
          <>
            {into}
            <button
              type="button"
              onClick={() => setEditingPath(tildify(path, home))}
              className="set-link"
              data-testid="project-clone-change"
            >
              change
            </button>
            {shownTarget?.cloned && (
              <span className="d-chip" title="Already cloned there: reused" data-testid="project-clone-reused">
                reused
              </span>
            )}
          </>
        ) : (
          <span className="set-faint">https://, ssh:// or git@host:path</span>
        )}
      </div>
      {error && (
        <span className="set-cform__err" title={error} data-testid="project-clone-error">
          {error}
        </span>
      )}
    </div>
  );
}
