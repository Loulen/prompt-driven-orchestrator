import { Plus } from "lucide-react";
import type { RunsPaneState } from "../../store/runsList/useRunsPane";
import { ExtensionActionButtons } from "../../../shared/extensions/ExtensionActions";
import { Button, IconButton, Tooltip } from "../../../shared/ui/kit";
import LeftPaneBar from "../../../shared/ui/LeftPaneBar";
import {
  useRecheckRunCreationRights,
  useRunCreationRightsStore,
} from "../../store/runCreationRightsStore";
import { canCreateSomeRun, NO_RUN_ANYWHERE } from "../../model/runCreationRights";


const NO_EXTRA = {};

interface Props {
  pane: RunsPaneState;
  onNewRun: () => void;
}

/** The Runs tab's header: the extensions' `runs/header` actions (#30), the
 *  « Project » configurator and New run. */
export default function RunsPaneHeader({ pane, onNewRun }: Props) {
  const { availableRepos, projectOf, openProjectSettings, canOpenProjectSettings } = pane;
  // CORE-5: disabled, with its reason, when no Projet allows creating a Run.
  const canCreate = useRunCreationRightsStore((s) => canCreateSomeRun(s.rights));
  useRecheckRunCreationRights(!canCreate);
  return (
    <LeftPaneBar title="Runs">
      {/* #30 — `runs/header`: the extensions' icon buttons, before the core's. */}
      <ExtensionActionButtons
        location="runs/header"
        extra={NO_EXTRA}
        className="flex items-center gap-px"
        buttonClassName="d-icon"
        iconSize={14}
      />
      {availableRepos.length > 0 && canOpenProjectSettings && (
        <Button
          size="sm"
          tone="ghost"
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
        >
          Project
        </Button>
      )}
      {canCreate ? (
        <IconButton icon={Plus} label="New run" onClick={onNewRun} data-testid="new-run-button" />
      ) : (
        <Tooltip label="New run" description={NO_RUN_ANYWHERE}>
          {/* A disabled button fires no pointer event: the span carries the tooltip. */}
          <span className="inline-flex" tabIndex={0} data-testid="new-run-disabled">
            <button type="button" disabled aria-label="New run" data-testid="new-run-button" className="d-icon">
              <Plus size={14} strokeWidth={1.5} aria-hidden />
            </button>
          </span>
        </Tooltip>
      )}
    </LeftPaneBar>
  );
}
