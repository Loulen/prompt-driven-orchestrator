import { useMemo, useState } from "react";
import { useRefusal } from "../../../shared/authorization/useRefusals";
import { Briefcase, FolderPlus, Globe, Plus } from "lucide-react";
import RepoCombobox from "../../../workspace/components/RepoCombobox";
import SecondaryRepoRow from "../SecondaryRepoRow";
import SourceBranchField from "../../../workspace/components/SourceBranchField";
import ProjectEditModal from "../../../workspace/components/ProjectEditModal";
import { useProjectRights } from "../../../workspace/store/useProjectRights";
import type { NewRunForm } from "../../store/newRun/useNewRunForm";
import type { Project } from "../../../workspace/model/project";
import { useRunCreationRightsStore } from "../../store/runCreationRightsStore";
import { hiddenProjectsNote, isRestricted, repoGroups } from "../../model/runCreationRights";
import { projectRepoGroups } from "../../model/launchProject";
import { isRemoteUrl } from "../../../workspace/model/clone";
import { Button } from "../../../shared/ui/kit";
import FormRow from "../../../shared/ui/FormRow";
import MidPath from "../../../shared/ui/MidPath";
import { tildify } from "../../../lib/homePath";
import ProjectCombobox from "./ProjectCombobox";
import RemoteRepoHint from "./RemoteRepoHint";

/** The « default » pill beside the Projet's default repository: never over the path. */
function DefaultPill({ of, testId }: { of: string; testId?: string }) {
  return (
    <span className="d-chip ln-pill" title={`Default repository of ${of}`} data-testid={testId}>
      default
    </span>
  );
}

/**
 * ── WHERE ── the Projet the Run is launched in (ADR-0087), its target
 * repository (the Projet's default primary, preselected; or a URL, cloned
 * first, CORE-97), the source branch, then the Projet's other repositories as
 * checkable secondaries, read-only by default, and the secondaries outside
 * the Projet (#465). With no Projet yet, one can be created from here.
 */
export default function WhereSection({ form, repoExplorerStartPath }: { form: NewRunForm; repoExplorerStartPath?: string }) {
  const {
    branches, branchesLoading, sourceBranch, setSourceBranch, lastFetchAt, branchFetchError,
    branchesFetching, refetchRemotes, rereadBranches, fastForward, targetRepo, repoValid,
    repoError, repoValidating, handleRepoChange, mode, secondaryRepos,
    recentRepos, updateSecondary, removeSecondary, addSecondary, settings,
    offeredProjects, allowNoProject, launchProject, projectNote, chooseProject, projectRepos,
    toggleProjectSecondary, outsideProjectReason, projectId, remoteUrl, projects, loadPipelines,
    searchOnly, pickRepo, projectFromRepo, ambiguousReason,
  } = form;
  // CORE-5: where the caller may create a Run. Restricted, the field offers the
  // repositories of the allowed Projets only, and the loupe is greyed, with
  // why, when the disk may not be browsed (CORE-205).
  const rights = useRunCreationRightsStore((s) => s.rights);
  const rightsProjects = useRunCreationRightsStore((s) => s.projects);
  const refreshRights = useRunCreationRightsStore((s) => s.refresh);
  const restricted = isRestricted(rights);
  const home = settings?.home ?? null;
  const isProjectDefault =
    launchProject != null && targetRepo.trim() !== "" && launchProject.primary_repo === targetRepo.trim();
  // In a Projet, the field offers its repositories, then the recents; in none,
  // the recents (or, restricted, the repositories of the allowed Projets).
  // Search-only, the repositories of the chosen Projet, else of every Projet
  // offered, nothing else.
  const groups = searchOnly
    ? projectRepoGroups(offeredProjects, launchProject)
    : launchProject
    ? [
        { label: launchProject.name, repos: launchProject.members },
        ...(restricted ? [] : [{ label: "Recent", repos: recentRepos.filter((r) => !launchProject.members.includes(r)) }]),
      ]
    : restricted
      ? repoGroups(rights, recentRepos, rightsProjects)
      : undefined;
  // A secondary that is one of the Projet's repositories shows among them,
  // whichever way it was added; the others are « outside » the Projet.
  const projectPaths = new Set(projectRepos);
  const outside = secondaryRepos
    .map((repo, index) => ({ repo, index }))
    .filter(({ repo }) => !projectPaths.has(repo.path.trim()));
  const showProject = offeredProjects.length > 0 || projectId !== null;
  // No Projet at all yet: one may be created here, over the form.
  const noProjectYet = !showProject && projects !== null && projects.length === 0;
  const projectRightsAnswer = useProjectRights(projects ?? [], noProjectYet);
  const [creating, setCreating] = useState(false);
  // The other Projets holding a repository (ADR-0087): than the one chosen,
  // or than the group it is listed under.
  const alsoIn = (path: string, besides = launchProject?.id) =>
    offeredProjects.filter((p) => p.id !== besides && p.members.includes(path)).map((p) => p.name);
  const alsoMark = (others: string[]) =>
    others.length > 0 ? (
      <span className="ln-faint ln-also" title={`Also in ${others.join(", ")}`} data-testid="repo-also-in">
        +{others.length}
      </span>
    ) : null;
  // In no Projet (the run mode's Launch), a repository several Projets hold.
  const ambiguity = mode === "run" ? ambiguousReason : undefined;
  const typedPath = targetRepo.trim() && !isRemoteUrl(targetRepo.trim()) ? [targetRepo.trim()] : [];
  // CORE-205: the fast-forward greyed, with why, when the caller may not move the
  // branch: asked in the Projets holding the repository, as the daemon asks it.
  const repoPath = targetRepo.trim();
  const fastForwardQuestion = useMemo(
    () => ({
      resource: "repository",
      verb: { action: "fast_forward" },
      target: repoPath,
      projects: (projects ?? []).filter((p) => p.members.includes(repoPath)).map((p) => p.id),
    }),
    [repoPath, projects],
  );
  const fastForwardRefused = useRefusal(fastForwardQuestion, repoValid === true && !!repoPath);

  return (
    <>
      {/* Project (ADR-0087): first, it presets the rest. */}
      {showProject ? (
        <FormRow label="Project" htmlFor="launch-project">
          <ProjectCombobox
            value={projectId}
            onChange={chooseProject}
            projects={offeredProjects}
            allowNone={allowNoProject}
            note={projectNote}
            fromRepo={projectFromRepo}
          />
        </FormRow>
      ) : (
        noProjectYet && (
          <FormRow label="Project">
            <span className="ln-faint ln-none">
              <Briefcase size={13} strokeWidth={1.5} aria-hidden />
              No project yet
            </span>
            <Button
              size="sm"
              icon={FolderPlus}
              onClick={() => setCreating(true)}
              disabledReason={projectRightsAnswer.why.create}
              data-testid="launch-new-project"
            >
              New project
            </Button>
          </FormRow>
        )
      )}

      {/* Target repository (primary — target_repos[0], #465) */}
      <FormRow label="Repo" htmlFor="target-repo" title="Target repository">
        <div className="ln-grow">
          <RepoCombobox
            ariaLabel="Target repository"
            value={targetRepo}
            onChange={handleRepoChange}
            recentRepos={recentRepos}
            repoValid={repoValid}
            repoValidating={repoValidating}
            repoError={repoError}
            explorerStartPath={repoExplorerStartPath}
            browseRefused={rights.browseRefused}
            groups={groups}
            groupsNote={!launchProject && restricted && !searchOnly ? hiddenProjectsNote(rights) : null}
            searchOnly={searchOnly}
            onPick={searchOnly ? pickRepo : undefined}
            home={home}
            // CORE-97: a URL is cloned first, then launched on.
            leading={remoteUrl ? <Globe size={13} strokeWidth={1.5} className="ln-field__lead ln-field__lead--url" aria-hidden /> : undefined}
            hint={remoteUrl ? <RemoteRepoHint clone={form} home={home} /> : undefined}
            badge={isProjectDefault ? <DefaultPill of={launchProject.name} testId="project-default-badge" /> : undefined}
            rowBadge={(path, group) => (
              <>
                {launchProject?.primary_repo === path && group.projectId === launchProject.id && (
                  <DefaultPill of={launchProject.name} />
                )}
                {group.projectId && alsoMark(alsoIn(path, group.projectId))}
              </>
            )}
            refusal={
              outsideProjectReason && !remoteUrl ? (
                <span className="ln-err" data-testid="primary-outside-project">
                  {outsideProjectReason}
                </span>
              ) : ambiguity && !remoteUrl ? (
                <span className="ln-err" data-testid="primary-ambiguous">
                  {ambiguity}
                </span>
              ) : undefined
            }
          />
        </div>
      </FormRow>

      {/* Source branch */}
      {repoValid && (
        <FormRow label="Branch" htmlFor="source-branch" title={mode === "trigger" ? "Each fire cuts from this branch" : "Source branch"}>
          {/* #802/ADR-0070: the quick pick renders the held value verbatim, so the
              switch to an unlisted `origin/<x>` is safe. #804: in Trigger mode a
              local branch earns a warning — every fire cuts from its local state. */}
          <div className="ln-branch">
            <SourceBranchField
              purpose={mode === "trigger" ? "trigger" : "run"}
              id="source-branch"
              ariaLabel="Source branch"
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
              fastForwardRefused={fastForwardRefused}
              testIdPrefix="source-branch"
            />
          </div>
        </FormRow>
      )}

      {/* The Projet's other repositories (ADR-0087): checkable secondaries,
          read-only by default. */}
      {repoValid && launchProject && projectRepos.length > 0 && (
        <FormRow label="Also" top title={`Other repositories of ${launchProject.name}`} testId="project-repos">
          <div className="ln-secs">
            {projectRepos.map((path) => {
              const index = secondaryRepos.findIndex((r) => r.path.trim() === path);
              const also = alsoMark(alsoIn(path));
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
                    also={also}
                    inProject
                  />
                );
              }
              return (
                <label key={path} className="ln-secrow__line" data-testid="project-repo-unchecked" data-path={path}>
                  <input
                    type="checkbox"
                    checked={false}
                    onChange={() => toggleProjectSecondary(path, true)}
                    aria-label={`Use ${path} as a secondary repository`}
                  />
                  <MidPath path={tildify(path, home)} className="ln-secrow__path" />
                  {also}
                </label>
              );
            })}
          </div>
        </FormRow>
      )}

      {/* Secondary repositories outside the Projet (#465 / ADR-0042), each row
          self-validating, with its own base branch. */}
      {repoValid &&
        outside.map(({ repo, index }) => (
          <FormRow key={index} label={<span className="ln-faint">+ repo</span>} top title="Secondary repository">
            <div className="ln-grow">
              <SecondaryRepoRow
                index={index}
                repo={repo}
                recentRepos={recentRepos.filter((r) => r !== targetRepo.trim())}
                onChange={updateSecondary}
                onRemove={removeSecondary}
                home={home}
              />
            </div>
          </FormRow>
        ))}
      {/* A repository outside the Projet is read from the disk: search-only, none. */}
      {repoValid && !searchOnly && (
        <FormRow label="">
          <Button
            size="sm"
            tone="ghost"
            icon={Plus}
            className="ln-add"
            onClick={addSecondary}
            title={launchProject ? `Add a repository outside ${launchProject.name}` : "Add a repository"}
            data-testid="add-secondary-repo"
          >
            <span className="ln-ellipsis">{launchProject ? `Repository outside ${launchProject.name}` : "Repository"}</span>
          </Button>
        </FormRow>
      )}

      {creating && (
        <ProjectEditModal
          initialProject={null}
          initialName=""
          initialMemberPaths={typedPath}
          availableRepos={recentRepos}
          projects={projects ?? []}
          home={home}
          onClose={() => setCreating(false)}
          onSaved={(project?: Project) => {
            void loadPipelines().then(() => {
              void refreshRights();
              if (project) chooseProject(project.id);
            });
          }}
        />
      )}
    </>
  );
}
