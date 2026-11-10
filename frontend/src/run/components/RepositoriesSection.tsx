import { useMemo, useState } from "react";
import { CloudOff, FolderGit2, GitBranch, X, Plus } from "lucide-react";
import { Banner, Button, Chip, EmptyState, IconButton, Section } from "../../shared/ui/kit";
import "../../pipeline/components/inspect.css";
import type { RunState } from "../../shared/generated/RunState";
import { editRunRepos } from "../api/createRun";
import { useRecentReposStore } from "../../stores/recentReposStore";
import SecondaryRepoRow, { type SecondaryRepo } from "./SecondaryRepoRow";
import { SourceDriftChip } from "../../pipeline/components/InspectorPrimitives";
import { useSourceDrift } from "../store/useSourceDrift";
import { useRefusal } from "../../shared/authorization/useRefusals";

/** A Run is editable (its repo list can change) exactly while it is live — the same
 *  set the server's `is_live` enforces. A terminal Run's list is frozen. */
const LIVE_STATUSES: ReadonlySet<string> = new Set([
  "running",
  "awaiting_user",
  "paused",
]);

/**
 * The **Repositories** tab of the Run panel (#752, closing the second half of
 * #566): the primary + secondary repositories of a Run, with mid-run add/remove
 * of secondaries (#465 slice 2, ADR-0042). It used to be a section of the
 * standalone `RunInfoSidebar` — mounted only when nothing was selected on the
 * canvas, reached through a toolbar toggle; both are gone. The content is
 * unchanged: primary locked, secondaries editable on a live Run (add / remove,
 * writable or read-only per ADR-0047), frozen on a terminal or archived Run,
 * inline error strip, spawn-visibility note. Editing here is **spawn-time
 * visible**: a change touches nodes launched after it, never the ones running.
 */
export default function RepositoriesSection({
  run,
  onEdited,
}: {
  run: RunState;
  onEdited?: () => void;
}) {
  const editable = LIVE_STATUSES.has(run.status);
  // CORE-205: why the caller may not edit the Run's repositories (its gestures greyed), else absent.
  const editQuestion = useMemo(
    () => ({ resource: "run", verb: { action: "edit_repos" }, target: run.run_id }),
    [run.run_id],
  );
  const editRefused = useRefusal(editQuestion, editable);
  const recentRepos = useRecentReposStore((s) => s.recentRepos);
  // #803: the same measurement the Info tab's Source block shows, from the same
  // hook — two renderings of one number, never two computations of it.
  const { drift, fetchError } = useSourceDrift(run);

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
    <div data-testid="run-repositories">
      {/* Primary — locked, read-only. It never has an alias, so it can be neither
          removed nor re-pointed mid-run.

          #803/#804: its second line is the cut point the Run itself knows,
          marked when a Trigger's pre-cut fetch failed (#804), and the drift chip
          (#803). Only the primary has a chip: a secondary is pinned to a SHA at
          add time, so it has no « Run branch » to drift from. */}
      <Section title="Primary">
        <div className="pi-repo" data-testid="primary-repo-row">
          <FolderGit2 size={14} strokeWidth={1.5} aria-hidden className="ei-ic" />
          <span className="d-mono ei-trunc pi-repo__path" title={run.target_repo ?? ""}>
            {run.target_repo}
          </span>
          <SourceDriftChip drift={drift} runId={run.run_id} fetchError={fetchError} testid="primary-repo-drift" />
          <span data-testid="primary-repo-badge">
            <Chip mono>PRIMARY</Chip>
          </span>
        </div>
        <div className="pi-repo__cut d-mono" data-testid="primary-repo-cut">
          <GitBranch size={11} strokeWidth={1.5} aria-hidden className="ei-ic" />
          {run.source_branch ?? "HEAD"}
          {run.fork_sha && ` · ${run.fork_sha.slice(0, 8)}`}
          {/* Icon only, reason on hover: the Info tab says it in full. */}
          {run.source_fetch_error && (
            <span
              role="img"
              className="ei-line"
              style={{ color: "var(--color-st-skipped)" }}
              title={`Fetch failed before the cut — cut from local state. ${run.source_fetch_error.message}`}
              aria-label={`Fetch failed before the cut — cut from local state. ${run.source_fetch_error.message}`}
              data-testid="primary-repo-fetch-failed"
            >
              <CloudOff size={11} strokeWidth={1.5} aria-hidden />
            </span>
          )}
        </div>
      </Section>

      {/* Existing secondaries — locked display + a remove button, with a badge
          for the writable/read-only mode (ADR-0047). Their SHA is frozen at add,
          so there is nothing to edit here but their presence. */}
      <Section
        title="Secondary"
        count={secondaries.length}
        flush
        aside={
          editable && draft === null ? (
            <IconButton
              icon={Plus}
              size="sm"
              label="Add repository"
              description="Applies to nodes launched after the change"
              onClick={() => setDraft({ path: "", baseBranch: "", valid: null, readOnly: false })}
              disabled={busy && !editRefused}
              disabledReason={editRefused}
              data-testid="add-secondary-repo"
            />
          ) : undefined
        }
      >
        {secondaries.length === 0 && draft === null && (
          <EmptyState icon={FolderGit2} layout="row">
            None
          </EmptyState>
        )}
        {secondaries.map((pin) => (
          <div key={pin.alias} className="pi-repo pi-repo--two" data-testid={`secondary-repo-${pin.alias}`}>
            <FolderGit2 size={14} strokeWidth={1.5} aria-hidden className="ei-ic" />
            <span className="pi-repo__text">
              <span className="d-mono ei-trunc" title={pin.repo}>
                {pin.repo}
              </span>
              <span className="pi-repo__cut d-mono">
                <GitBranch size={11} strokeWidth={1.5} aria-hidden />
                {pin.base_branch ?? "HEAD"} · {pin.sha.slice(0, 8)}
              </span>
            </span>
            {/* ADR-0047: the badge reflects the per-repo opt-in. */}
            <span data-testid={`secondary-repo-mode-${pin.alias}`}>
              <Chip mono>{pin.read_only ? "READ-ONLY" : "WRITABLE"}</Chip>
            </span>
            {editable && (
              <IconButton
                icon={X}
                size="sm"
                label={`Remove secondary repository ${pin.alias}`}
                onClick={() => removeSecondary(pin.alias)}
                disabled={busy && !editRefused}
                disabledReason={editRefused}
                data-testid={`remove-secondary-repo-${pin.alias}`}
              />
            )}
          </div>
        ))}

        {/* The draft row self-validates; confirming fires a per-action
            `editRunRepos({ add })`. */}
        {editable && draft !== null && (
          <div className="pi-repo__draft" data-testid="secondary-repo-draft">
            <SecondaryRepoRow
              index={0}
              repo={draft}
              recentRepos={recentRepos}
              onChange={(_, patch) => setDraft((prev) => (prev ? { ...prev, ...patch } : prev))}
              onRemove={() => setDraft(null)}
            />
            <Button
              size="sm"
              tone="primary"
              onClick={addDraft}
              disabled={busy || draft.valid !== true || !draft.path.trim()}
              data-testid="confirm-add-secondary-repo"
            >
              Add repository
            </Button>
          </div>
        )}
      </Section>

      {error && (
        <div data-testid="run-repos-error">
          <Banner tone="failed" title="Not changed" text={error} />
        </div>
      )}

      {editable && (
        // Editing here is spawn-time visible: a change touches nodes launched after it.
        <span className="d-sr" data-testid="spawn-visibility-note">
          Applies to nodes launched after this change; running nodes keep their current context.
        </span>
      )}
    </div>
  );
}
