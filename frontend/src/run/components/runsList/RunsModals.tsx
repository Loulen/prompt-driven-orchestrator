import { isLiveRun } from "../../model/runStatus";
import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import type { Project } from "../../../workspace/model/project";
import { cascadeImpact } from "../../model/runTree";
import CleanupConfirmModal from "../CleanupConfirmModal";
import ForgetRunModal from "../ForgetRunModal";
import ProjectEditModal from "../../../workspace/components/ProjectEditModal";
import RunShellModal from "../../../session/components/RunShellModal";
import RetryAllConfirmModal from "./RetryAllConfirmModal";
import type { RunsPaneState } from "../../store/runsList/useRunsPane";

interface Props {
  pane: RunsPaneState;
  runs: RunListEntry[];
  projects: Project[];
  onProjectsChanged?: () => void;
}

/**
 * The Runs tab's overlays: the per-row confirms, the run shell and the Projet
 * editor. Rendered at the panel's root, whatever the active tab.
 */
export default function RunsModals({ pane, runs, projects, onProjectsChanged }: Props) {
  const {
    confirmCleanup, setConfirmCleanup, handleCleanup, confirmForget, setConfirmForget, handleForget,
    runShell, projectEditor, setProjectEditor, availableRepos, confirmRetryAll, setConfirmRetryAll,
    handleRetryAll,
  } = pane;
  return (
    <>
      {confirmCleanup && (
        <CleanupConfirmModal
          runId={confirmCleanup.runId}
          isLive={
            isLiveRun(confirmCleanup.status)
          }
          cascade={cascadeImpact(runs, [confirmCleanup.runId])}
          onConfirm={() => handleCleanup(confirmCleanup.runId)}
          onCancel={() => setConfirmCleanup(null)}
        />
      )}

      {confirmForget && (
        <ForgetRunModal
          onConfirm={() => handleForget(confirmForget)}
          onCancel={() => setConfirmForget(null)}
        />
      )}

      {runShell.shell && (
        <RunShellModal session={runShell.shell.session} onClose={runShell.close} />
      )}

      {projectEditor && (
        <ProjectEditModal
          initialProject={projectEditor.project}
          initialName={projectEditor.name}
          initialMemberPaths={projectEditor.memberPaths}
          availableRepos={availableRepos}
          projects={projects}
          onClose={() => setProjectEditor(null)}
          onSaved={() => onProjectsChanged?.()}
        />
      )}

      {confirmRetryAll && (
        <RetryAllConfirmModal
          onConfirm={() => handleRetryAll(confirmRetryAll)}
          onCancel={() => setConfirmRetryAll(null)}
        />
      )}
    </>
  );
}
