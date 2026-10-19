import { Briefcase, FolderGit2, GitBranch } from "lucide-react";
import RepoCombobox from "../../../workspace/components/RepoCombobox";
import SecondaryRepoRow, { SecondaryRepoLabel } from "../SecondaryRepoRow";
import SourceBranchField from "../../../workspace/components/SourceBranchField";
import type { NewRunForm } from "../../store/newRun/useNewRunForm";
import { useRunCreationRightsStore } from "../../store/runCreationRightsStore";
import { hiddenProjectsNote, isRestricted, repoGroups } from "../../model/runCreationRights";
import { tildify } from "../../../lib/homePath";
import ProjectCombobox from "./ProjectCombobox";

const LABEL = "font-medium text-fg-2 flex items-center gap-1.5";

/**
 * ── WHERE ── the Projet the Run is launched in (ADR-0087), its target
 * repository (the Projet's default primary, preselected), the source branch,
 * then the Projet's other repositories as secondaries, read-only by default,
 * and the secondaries outside the Projet (#465).
 */
export default function WhereSection({ form, repoExplorerStartPath }: { form: NewRunForm; repoExplorerStartPath?: string }) {
  const {
    branches, branchesLoading, sourceBranch, setSourceBranch, lastFetchAt, branchFetchError,
    branchesFetching, refetchRemotes, rereadBranches, fastForward, targetRepo, repoValid,
    repoError, repoValidating, repoBorderClass, handleRepoChange, mode, secondaryRepos,
    recentRepos, updateSecondary, removeSecondary, addSecondary, settings,
    offeredProjects, allowNoProject, launchProject, projectNote, chooseProject, projectRepos,
    toggleProjectSecondary, outsideProjectReason, projectId,
  } = form;
  // CORE-5: where the caller may create a Run. Restricted, the field offers the
  // repositories of the allowed Projets only, and the loupe goes when the disk
  // may not be browsed.
  const rights = useRunCreationRightsStore((s) => s.rights);
  const projects = useRunCreationRightsStore((s) => s.projects);
  const restricted = isRestricted(rights);
  const home = settings?.home ?? null;
  const isProjectDefault =
    launchProject != null && targetRepo.trim() !== "" && launchProject.primary_repo === targetRepo.trim();
  // In a Projet, the field offers its repositories; in none, the recents (or,
  // restricted, the repositories of the allowed Projets).
  const groups = launchProject
    ? [{ label: launchProject.name, repos: launchProject.members }]
    : restricted
      ? repoGroups(rights, recentRepos, projects)
      : undefined;
  // A secondary that is one of the Projet's repositories shows among them,
  // whichever way it was added; the others are « outside » the Projet.
  const projectPaths = new Set(projectRepos);
  const outside = secondaryRepos
    .map((repo, index) => ({ repo, index }))
    .filter(({ repo }) => !projectPaths.has(repo.path.trim()));
  const showProject = offeredProjects.length > 0 || projectId !== null;
  return (
    <div className="flex flex-col gap-3 pb-4 border-b border-line">
      <span className="text-fg-4 uppercase tracking-wider font-medium" style={{ fontSize: "10px" }}>
        Where
      </span>

      {/* Project (ADR-0087): first, it presets the rest. */}
      {showProject && (
        <div className="flex flex-col gap-1.5">
          <label htmlFor="launch-project" className={LABEL} style={{ fontSize: "11.5px" }}>
            <Briefcase size={12} className="text-fg-3" />
            Project
          </label>
          <ProjectCombobox
            value={projectId}
            onChange={chooseProject}
            projects={offeredProjects}
            allowNone={allowNoProject}
            note={projectNote}
          />
        </div>
      )}

      {/* Target repository (primary — target_repos[0], #465) */}
      <div className="flex flex-col gap-1.5">
        <label htmlFor="target-repo" className={LABEL} style={{ fontSize: "11.5px" }}>
          <FolderGit2 size={12} className="text-fg-3" />
          Target repository
          {(secondaryRepos.length > 0 || launchProject) && (
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
          canBrowse={rights.canBrowse}
          groups={groups}
          groupsNote={!launchProject && restricted ? hiddenProjectsNote(rights) : null}
          help={rights.canBrowse ? null : "A new repository goes through an administrator."}
          home={home}
          badge={
            isProjectDefault ? (
              <span
                className="rounded bg-acc/15 px-1.5 py-0.5 font-mono text-acc"
                style={{ fontSize: "9.5px" }}
                data-testid="project-default-badge"
              >
                project default
              </span>
            ) : undefined
          }
        />
        {outsideProjectReason && (
          <span className="text-st-failed" style={{ fontSize: "10.5px" }} data-testid="primary-outside-project">
            {outsideProjectReason}
          </span>
        )}
      </div>

      {/* Source branch */}
      {repoValid && (
        <div className="flex flex-col gap-1.5">
          <label htmlFor="source-branch" className={LABEL} style={{ fontSize: "11.5px" }}>
            <GitBranch size={12} className="text-fg-3" />
            Source branch
          </label>
          {/* #802/ADR-0070: the quick pick renders the held value verbatim, so the
              switch to an unlisted `origin/<x>` is safe. #804: in Trigger mode a
              local branch earns a warning — every fire cuts from its local state. */}
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

      {/* The Projet's other repositories (ADR-0087): checkable secondaries,
          read-only by default. */}
      {repoValid && launchProject && projectRepos.length > 0 && (
        <div className="flex flex-col gap-1.5" data-testid="project-repos">
          <span className="font-medium text-fg-2" style={{ fontSize: "11.5px" }}>
            Other repositories of {launchProject.name}
          </span>
          {projectRepos.map((path) => {
            const index = secondaryRepos.findIndex((r) => r.path.trim() === path);
            if (index >= 0) {
              return (
                <SecondaryRepoRow
                  key={path}
                  index={index}
                  repo={secondaryRepos[index]}
                  recentRepos={recentRepos}
                  onChange={updateSecondary}
                  onRemove={removeSecondary}
                  home={home}
                  inProject
                />
              );
            }
            return (
              <label
                key={path}
                className="flex cursor-pointer items-center gap-2 rounded-md border border-line bg-bg-3/60 px-2.5 py-1.5 transition-colors hover:border-line-strong"
                data-testid="project-repo-unchecked"
                data-path={path}
              >
                <input
                  type="checkbox"
                  className="accent-acc"
                  checked={false}
                  onChange={() => toggleProjectSecondary(path, true)}
                  aria-label={`Use ${path} as a secondary repository`}
                />
                <span className="min-w-0 flex-1 truncate font-mono text-fg-3" style={{ fontSize: "12px" }} title={path}>
                  {tildify(path, home)}
                </span>
              </label>
            );
          })}
        </div>
      )}

      {/* Secondary repositories outside the Projet (read-only opt-in, #465 /
          ADR-0042). Shown once a primary is chosen; each row self-validates and
          carries its own base-branch select. */}
      {repoValid && (
        <div className="flex flex-col gap-2">
          {outside.length > 0 && <SecondaryRepoLabel />}
          {outside.map(({ repo, index }) => (
            <SecondaryRepoRow
              key={index}
              index={index}
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
            {launchProject ? `+ Add a repository outside ${launchProject.name}` : "+ Add repository"}
          </button>
        </div>
      )}
    </div>
  );
}
