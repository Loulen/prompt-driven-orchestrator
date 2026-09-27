import { FolderGit2, GitBranch } from "lucide-react";
import RepoCombobox from "../../../workspace/components/RepoCombobox";
import SecondaryRepoRow, { SecondaryRepoLabel } from "../SecondaryRepoRow";
import SourceBranchField from "../../../workspace/components/SourceBranchField";
import type { NewRunForm } from "../../store/newRun/useNewRunForm";

/** ── WHERE ── the target repository, its source branch and the secondary repos (#465). */
export default function WhereSection({ form, repoExplorerStartPath }: { form: NewRunForm; repoExplorerStartPath?: string }) {
  const {
    branches, branchesLoading, sourceBranch, setSourceBranch, lastFetchAt, branchFetchError,
    branchesFetching, refetchRemotes, rereadBranches, fastForward, targetRepo, repoValid,
    repoError, repoValidating, repoBorderClass, handleRepoChange, mode, secondaryRepos,
    recentRepos, updateSecondary, removeSecondary, addSecondary,
  } = form;
  return (
    <div className="flex flex-col gap-3 pb-4 border-b border-line">
      <span className="text-fg-4 uppercase tracking-wider font-medium" style={{ fontSize: "10px" }}>
        Where
      </span>

      {/* Target repository (primary — target_repos[0], #465) */}
      <div className="flex flex-col gap-1.5">
        <label
          htmlFor="target-repo"
          className="font-medium text-fg-2 flex items-center gap-1.5"
          style={{ fontSize: "11.5px" }}
        >
          <FolderGit2 size={12} className="text-fg-3" />
          Target repository
          {secondaryRepos.length > 0 && (
            <span
              className="rounded bg-bg-4 px-1.5 py-0.5 font-medium text-fg-3"
              style={{ fontSize: "9.5px" }}
              data-testid="primary-repo-badge"
            >
              PRIMARY
            </span>
          )}
        </label>
        <RepoCombobox
          value={targetRepo}
          onChange={handleRepoChange}
          recentRepos={recentRepos}
          repoValid={repoValid}
          repoValidating={repoValidating}
          repoError={repoError}
          borderClass={repoBorderClass}
          explorerStartPath={repoExplorerStartPath}
        />
      </div>

      {/* Source branch */}
      {repoValid && (
        <div className="flex flex-col gap-1.5">
          <label
            htmlFor="source-branch"
            className="font-medium text-fg-2 flex items-center gap-1.5"
            style={{ fontSize: "11.5px" }}
          >
            <GitBranch size={12} className="text-fg-3" />
            Source branch
          </label>
          {/* #802/ADR-0070: the `<select>` is gone. It could show neither how
              old a branch was nor how far behind its upstream, and its
              value-not-in-options fallback was the #454 shows-one-sends-another
              trap. The quick pick renders the held value verbatim, so the
              switch to an unlisted `origin/<x>` (the popover's shortcut) is
              safe — a `<select>` could not have expressed it at all. */}
          {/* #804: the same field, told what it is choosing FOR. In Trigger
              mode — creating one, or editing one through the list's pencil —
              a local branch earns a warning, because every fire will cut from
              its local state and the pre-fire fetch cannot move it. */}
          <SourceBranchField
            purpose={mode === "trigger" ? "trigger" : "run"}
            id="source-branch"
            value={sourceBranch}
            onChange={setSourceBranch}
            branches={branches}
            loading={branchesLoading}
            fetching={branchesFetching}
            lastFetchAt={lastFetchAt}
            fetchError={branchFetchError}
            onFetch={refetchRemotes}
            onReread={rereadBranches}
            onFastForward={fastForward}
            testIdPrefix="source-branch"
          />
        </div>
      )}

      {/* Secondary repositories (read-only, #465 / ADR-0042). Shown once a
          primary is chosen; each row self-validates and carries its own
          base-branch select. */}
      {repoValid && (
        <div className="flex flex-col gap-2">
          {secondaryRepos.length > 0 && <SecondaryRepoLabel />}
          {secondaryRepos.map((repo, i) => (
            <SecondaryRepoRow
              key={i}
              index={i}
              repo={repo}
              recentRepos={recentRepos}
              onChange={updateSecondary}
              onRemove={removeSecondary}
            />
          ))}
          <button
            type="button"
            onClick={addSecondary}
            className="self-start rounded-md border border-dashed border-line-strong bg-transparent px-2.5 py-1.5 font-medium text-fg-3 transition-colors hover:border-acc hover:text-acc"
            style={{ fontSize: "11.5px" }}
            data-testid="add-secondary-repo"
          >
            + Add repository
          </button>
        </div>
      )}
    </div>
  );
}
