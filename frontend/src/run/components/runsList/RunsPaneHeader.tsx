import { ChevronsDownUp, ChevronsUpDown, Plus } from "lucide-react";
import type { RunsPaneState } from "../../store/runsList/useRunsPane";
import { ExtensionActionButtons } from "../../../shared/extensions/ExtensionActions";
import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import { IconButton } from "../../../shared/ui/kit";
import LeftPaneBar from "../../../shared/ui/LeftPaneBar";
import {
  useRecheckRunCreationRights,
  useRunCreationRightsStore,
} from "../../store/runCreationRightsStore";
import { whyNoRunAnywhere } from "../../model/runCreationRights";
import RunFilterFunnel from "./RunFilterFunnel";

const NO_EXTRA = {};

interface Props {
  pane: RunsPaneState;
  runs: RunListEntry[];
  triggers: { id: string; name: string }[];
  onNewRun: () => void;
}

/** The Runs tab's header: the extensions' `runs/header` actions (#30), the
 *  filters' funnel (CORE-110), expand / collapse all and New run. The Projets
 *  are set up in Settings › Projects. */
export default function RunsPaneHeader({ pane, runs, triggers, onNewRun }: Props) {
  // CORE-5: greyed, with its reason, when no Projet allows creating a Run.
  const noRunReason = useRunCreationRightsStore((s) => whyNoRunAnywhere(s.rights));
  useRecheckRunCreationRights(noRunReason !== undefined);
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
      {(runs.length > 0 || pane.filterActive) && <RunFilterFunnel pane={pane} runs={runs} triggers={triggers} />}
      {/* #783 — expand / collapse every child Run: not a filter. */}
      {pane.allParentIds.length > 0 && (
        <IconButton
          icon={pane.allExpanded ? ChevronsDownUp : ChevronsUpDown}
          label={pane.allExpanded ? "Collapse all child runs" : "Expand all child runs"}
          data-testid="run-tree-toggle-all"
          data-state={pane.allExpanded ? "expanded" : "collapsed"}
          onClick={() => pane.setAllExpanded(!pane.allExpanded)}
        />
      )}
      {/* CORE-205: greyed (`aria-disabled`, still focusable), its tooltip says why. */}
      <IconButton
        icon={Plus}
        label="New run"
        onClick={onNewRun}
        disabledReason={noRunReason}
        data-testid="new-run-button"
      />
    </LeftPaneBar>
  );
}
