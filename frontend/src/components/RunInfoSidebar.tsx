import { useState } from "react";
import { FolderGit2, GitBranch, X, Plus } from "lucide-react";
import type { RunState } from "../types";
import { editRunRepos } from "../api";
import { useRecentReposStore } from "../stores/recentReposStore";
import SecondaryRepoRow, { type SecondaryRepo } from "./SecondaryRepoRow";

/** A Run is editable (its repo list can change) exactly while it is live — the same
 *  set the server's `is_live` enforces. A terminal Run's list is frozen. */
const LIVE_STATUSES: ReadonlySet<string> = new Set([
  "running",
  "awaiting_user",
  "paused",
]);

/**
 * Right-panel header shown when a run-scoped edit tab is open with nothing
 * selected on the canvas. For a live/completed run it notes that edits sync
 * back to the template; for an archived run (#315) the canvas is read-only
 * (its worktree + `pipeline.yaml` are gone and Save is disabled), so it says
 * so instead of the misleading "changes sync to template" note.
 *
 * A Run that ended non-green also states **why**, above the editing note (#503).
 * That is the panel a user reaches by clicking a red dot, and it used to talk
 * about pipeline editing while saying nothing at all about the failure.
 *
 * #465 slice 2 (ADR-0042): when the Run names a primary repo, it also hosts the
 * **Repositories** section — the primary locked, the read-only secondaries
 * editable on a live Run (add / remove). Editing here is **spawn-time visible**:
 * a change touches nodes launched after it, never the ones already running.
 */
export default function RunInfoSidebar({
  run,
  onEdited,
}: {
  run: RunState;
  /** Called after a successful add/remove so the parent re-fetches the Run. */
  onEdited?: () => void;
}) {
  const archived = run.status === "archived";
  return (
    <aside className="flex h-full flex-col bg-bg-2" style={{ fontSize: "12px" }}>
      <div className="border-b border-line px-3 py-3">
        <div className="font-medium text-fg">{run.pipeline_name}</div>
        <div className="mt-0.5 font-mono text-fg-4" style={{ fontSize: "10px" }}>
          {run.run_id}
        </div>
        {run.failure_reason && (
          <div
            className="mt-2 rounded border border-st-failed/30 bg-st-failed-bg px-2 py-1.5 text-fg-2"
            style={{ fontSize: "10.5px" }}
            data-testid="run-failure-reason"
          >
            <div className="font-medium text-st-failed">
              {run.status === "halted"
                ? "Halted"
                : run.status === "skipped"
                  ? "Skipped"
                  : "Failed"}
            </div>
            <div className="mt-0.5 break-words">{run.failure_reason}</div>
          </div>
        )}
        {/* #598 / ADR-0049: an incident-parked run is `awaiting_user` with an
            `awaiting_reason` — distinct from an interactive wait (no reason) and
            from a terminal failure (`failure_reason`). Surface it so the operator
            sees WHY the run parked and can Reopen/Retry it. */}
        {run.awaiting_reason && (
          <div
            className="mt-2 rounded border border-st-await/30 bg-st-await-bg px-2 py-1.5 text-fg-2"
            style={{ fontSize: "10.5px" }}
            data-testid="run-awaiting-reason"
          >
            <div className="font-medium text-st-await">Interrupted · awaiting you</div>
            <div className="mt-0.5 break-words">{run.awaiting_reason}</div>
          </div>
        )}
        <div
          className="mt-2 rounded border border-line-strong bg-bg-3 px-2 py-1.5 text-fg-3"
          style={{ fontSize: "10.5px" }}
          data-testid="run-info-note"
        >
          {archived
            ? "Archived run · read-only · outputs preserved"
            : "Editing run-scoped pipeline · changes sync to template"}
        </div>
        {/* #551 (ADR-0046): the harness this Run was FROZEN on — the `run` tier that
            made it an A/B of the same pipeline on another harness. Shown only when the
            Run named one; absence means it inherited the instance default (and the
            `claude` floor), which the panel does not need to spell out per-Run. */}
        {run.harness && (
          <div
            className="mt-2 flex items-center gap-1.5 text-fg-3"
            style={{ fontSize: "10.5px" }}
            data-testid="run-harness"
          >
            <span className="text-fg-4">Harness</span>
            <span className="rounded bg-bg-3 px-1.5 py-0.5 font-mono text-fg-2">
              {run.harness}
            </span>
          </div>
        )}
      </div>

      {run.target_repo && (
        <RepositoriesSection run={run} onEdited={onEdited} />
      )}
    </aside>
  );
}

/** The primary + secondary repositories of a Run, with mid-run add/remove of
 *  read-only secondaries (#465 slice 2). */
function RepositoriesSection({
  run,
  onEdited,
}: {
  run: RunState;
  onEdited?: () => void;
}) {
  const editable = LIVE_STATUSES.has(run.status);
  const recentRepos = useRecentReposStore((s) => s.recentRepos);

  // The draft "+ Add repository" row, when open. `null` = no row shown. It is a
  // self-validating `SecondaryRepoRow`; adding is a per-action `editRunRepos`, not a
  // batched launch (there is no "launch" on a live Run).
  const [draft, setDraft] = useState<SecondaryRepo | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const secondaries = run.target_repos ?? [];

  async function apply(body: Parameters<typeof editRunRepos>[1]) {
    setBusy(true);
    setError(null);
    try {
      const outcome = await editRunRepos(run.run_id, body);
      if (outcome.kind === "refused") {
        setError(outcome.message);
        return false;
      }
      onEdited?.();
      return true;
    } catch {
      setError("The edit could not be sent — the daemon may be unreachable.");
      return false;
    } finally {
      setBusy(false);
    }
  }

  async function removeSecondary(alias: string) {
    await apply({ remove: [alias] });
  }

  async function addDraft() {
    if (!draft || draft.valid !== true || !draft.path.trim()) return;
    const ok = await apply({
      add: [
        {
          repo: draft.path.trim(),
          base_branch: draft.baseBranch || undefined,
          read_only: draft.readOnly,
        },
      ],
    });
    if (ok) setDraft(null);
  }

  return (
    <div
      className="flex flex-col gap-2 border-b border-line px-3 py-3"
      data-testid="run-repositories"
    >
      <span
        className="font-medium text-fg-2 flex items-center gap-1.5"
        style={{ fontSize: "11.5px" }}
      >
        <FolderGit2 size={12} className="text-fg-3" />
        Repositories
      </span>

      {/* Primary — locked, read-only. It never has an alias, so it can be neither
          removed nor re-pointed mid-run. */}
      <div
        className="flex items-center gap-2 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1.5"
        data-testid="primary-repo-row"
      >
        <span className="flex-1 truncate font-mono text-fg" title={run.target_repo ?? ""}>
          {run.target_repo}
        </span>
        <span
          className="rounded bg-bg-4 px-1.5 py-0.5 font-medium text-fg-3"
          style={{ fontSize: "9.5px" }}
          data-testid="primary-repo-badge"
        >
          PRIMARY
        </span>
      </div>

      {/* Existing secondaries — locked display + a remove button, with a badge
          for the writable/read-only mode (ADR-0047). Their SHA is frozen at add,
          so there is nothing to edit here but their presence. */}
      {secondaries.map((pin) => (
        <div
          key={pin.alias}
          className="flex items-center gap-2 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1.5"
          data-testid={`secondary-repo-${pin.alias}`}
        >
          <div className="flex min-w-0 flex-1 flex-col">
            <span className="truncate font-mono text-fg" title={pin.repo}>
              {pin.repo}
            </span>
            <span
              className="flex items-center gap-1 font-mono text-fg-4"
              style={{ fontSize: "10px" }}
            >
              <GitBranch size={10} className="text-fg-4" />
              {pin.base_branch ?? "HEAD"} · {pin.sha.slice(0, 8)}
            </span>
          </div>
          {/* ADR-0047: the badge reflects the per-repo opt-in. A writable
              secondary (the default) gets a discreet WRITABLE badge; only an
              opted-in read-only pin shows READ-ONLY. */}
          <span
            className="rounded bg-bg-4 px-1.5 py-0.5 font-medium text-fg-3"
            style={{ fontSize: "9.5px" }}
            data-testid={`secondary-repo-mode-${pin.alias}`}
          >
            {pin.read_only ? "READ-ONLY" : "WRITABLE"}
          </span>
          {editable && (
            <button
              type="button"
              onClick={() => removeSecondary(pin.alias)}
              disabled={busy}
              className="rounded-md border border-line-strong bg-bg-3 p-1.5 text-fg-3 transition-colors hover:border-st-failed hover:text-st-failed disabled:opacity-40"
              aria-label={`Remove secondary repository ${pin.alias}`}
              data-testid={`remove-secondary-repo-${pin.alias}`}
            >
              <X size={12} />
            </button>
          )}
        </div>
      ))}

      {/* Add affordance — a live Run only. The draft row self-validates; confirming
          fires a per-action `editRunRepos({ add })`. */}
      {editable && draft !== null && (
        <div className="flex flex-col gap-2" data-testid="secondary-repo-draft">
          <SecondaryRepoRow
            index={0}
            repo={draft}
            recentRepos={recentRepos}
            onChange={(_, patch) => setDraft((prev) => (prev ? { ...prev, ...patch } : prev))}
            onRemove={() => setDraft(null)}
          />
          <button
            type="button"
            onClick={addDraft}
            disabled={busy || draft.valid !== true || !draft.path.trim()}
            className="self-start rounded-md border border-acc bg-transparent px-2.5 py-1.5 font-medium text-acc transition-colors hover:bg-acc hover:text-bg disabled:opacity-40"
            style={{ fontSize: "11.5px" }}
            data-testid="confirm-add-secondary-repo"
          >
            Add repository
          </button>
        </div>
      )}

      {editable && draft === null && (
        <button
          type="button"
          onClick={() =>
            setDraft({ path: "", baseBranch: "", valid: null, readOnly: false })
          }
          disabled={busy}
          className="flex items-center gap-1 self-start rounded-md border border-dashed border-line-strong bg-transparent px-2.5 py-1.5 font-medium text-fg-3 transition-colors hover:border-acc hover:text-acc disabled:opacity-40"
          style={{ fontSize: "11.5px" }}
          data-testid="add-secondary-repo"
        >
          <Plus size={12} />
          Add repository
        </button>
      )}

      {error && (
        <div
          className="rounded border border-st-failed/30 bg-st-failed-bg px-2 py-1.5 text-st-failed"
          style={{ fontSize: "10.5px" }}
          data-testid="run-repos-error"
        >
          {error}
        </div>
      )}

      {editable && (
        <p className="text-fg-4" style={{ fontSize: "10px" }} data-testid="spawn-visibility-note">
          Applies to nodes launched after this change; running nodes keep their
          current context.
        </p>
      )}
    </div>
  );
}
