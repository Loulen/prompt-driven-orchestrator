import { Plus } from "lucide-react";
import type { RunsPaneState } from "../../store/runsList/useRunsPane";
import { ExtensionActionButtons } from "../../../shared/extensions/ExtensionActions";
import { Tooltip } from "../../../shared/ui/tooltip";
import {
  useRecheckRunCreationRights,
  useRunCreationRightsStore,
} from "../../store/runCreationRightsStore";
import { canCreateSomeRun } from "../../model/runCreationRights";

/** Why « New Run » is disabled (CORE-5). */
export const NO_RUN_ANYWHERE = "You can't create Runs in any Project. Ask an administrator for access.";

const NO_EXTRA = {};

interface Props {
  pane: RunsPaneState;
  onNewRun: () => void;
}

/** The Runs tab's title bar: the extensions' `runs/header` actions (#30), the
 *  « Project » configurator and « New Run ». */
export default function RunsPaneHeader({ pane, onNewRun }: Props) {
  const { availableRepos, projectOf, openProjectSettings, canOpenProjectSettings } = pane;
  // CORE-5: disabled, with its reason, when no Projet allows creating a Run.
  const canCreate = useRunCreationRightsStore((s) => canCreateSomeRun(s.rights));
  useRecheckRunCreationRights(!canCreate);
  return (
    <div
      className="flex h-[32px] shrink-0 items-center border-b border-line px-3 font-medium text-fg-2"
      style={{ fontSize: "11.5px" }}
    >
      Runs
      <div className="ml-auto flex items-center">
      {/* #30 — `runs/header`: the extensions' icon buttons, before the core's. */}
      <ExtensionActionButtons
        location="runs/header"
        extra={NO_EXTRA}
        className="mr-1.5 flex items-center gap-0.5"
      />
      {availableRepos.length > 0 && canOpenProjectSettings && (
        <button
          type="button"
          aria-label="Configure project"
          onClick={() => {
            // CORE-96: Settings › Projects — on the one repository's Projet, or
            // naming one for it; with several repositories, the page as it is.
            if (availableRepos.length > 1) {
              openProjectSettings();
              return;
            }
            const path = availableRepos[0];
            const project = projectOf(path);
            openProjectSettings(
              project
                ? {
                    kind: "project",
                    key: `project:${project.id}`,
                    repoPath: "",
                    label: project.name,
                  }
                : {
                    kind: "path",
                    key: path,
                    repoPath: path,
                    label: path.split("/").filter(Boolean).pop() ?? path,
                  },
            );
          }}
          className="cursor-pointer rounded border border-line-strong bg-bg-3 px-1.5 py-0.5 text-fg-3 hover:border-acc hover:text-fg"
          style={{ fontSize: "10.5px" }}
        >
          Project
        </button>
      )}
      {canCreate ? (
        <button
          onClick={onNewRun}
          data-testid="new-run-button"
          className={`${availableRepos.length > 0 && canOpenProjectSettings ? "ml-1.5" : ""} flex cursor-pointer items-center gap-1 rounded bg-acc px-1.5 py-0.5 font-medium text-on-acc transition-colors hover:bg-acc-dim`}
          style={{ fontSize: "10.5px" }}
        >
          <Plus size={10} />
          New Run
        </button>
      ) : (
        <Tooltip content={NO_RUN_ANYWHERE}>
          {/* A disabled button fires no pointer event: the span carries the tooltip. */}
          <span
            className={`${availableRepos.length > 0 && canOpenProjectSettings ? "ml-1.5" : ""} inline-flex`}
            tabIndex={0}
            data-testid="new-run-disabled"
          >
            <button
              disabled
              data-testid="new-run-button"
              className="flex cursor-not-allowed items-center gap-1 rounded bg-acc px-1.5 py-0.5 font-medium text-on-acc opacity-50"
              style={{ fontSize: "10.5px" }}
            >
              <Plus size={10} />
              New Run
            </button>
          </span>
        </Tooltip>
      )}
      </div>
    </div>
  );
}
