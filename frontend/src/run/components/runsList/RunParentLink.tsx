import { CornerLeftUp } from "lucide-react";
import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import { EMPTY_RUN_FILTER } from "../../model/runFilter";
import type { RunsPaneState } from "../../store/runsList/useRunsPane";
import type { ElementMarks } from "../../../hooks/useElementMarks";

/** What the link reads of the Runs tab's state. */
export type RunParentLinkPane = Pick<
  RunsPaneState,
  "filteredRuns" | "setRunFilter" | "setAttributeFilter" | "setProjectFilter"
>;

interface Props {
  parent: RunListEntry;
  pane: RunParentLinkPane;
  onSelectRun: (runId: string) => void;
  /** Its marks as an Élément masquable (a preview's ring, its ghost). */
  marks?: ElementMarks;
}

/**
 * The link a root row carries to its parent when that parent sits in the other
 * section of the Runs list (active / archived — CONTEXT.md « Run racine »). The
 * click (or Enter / Space) selects the parent, whose section opens if need be;
 * a parent the filters hide brings them down first, or there would be no row
 * to reveal. Inside the row's button: it never selects the row itself.
 */
export default function RunParentLink({ parent, pane, onSelectRun, marks }: Props) {
  const { filteredRuns, setRunFilter, setAttributeFilter, setProjectFilter } = pane;
  const label = parent.name || parent.run_id;
  const goToParent = () => {
    if (!filteredRuns.some((r) => r.run_id === parent.run_id)) {
      setRunFilter(EMPTY_RUN_FILTER);
      setAttributeFilter({});
      setProjectFilter({});
    }
    onSelectRun(parent.run_id);
  };
  return (
    <span
      role="button"
      title={`Child of “${label}” (${parent.status === "archived" ? "archived" : "active"}) — click to select it`}
      aria-label={`Go to parent run ${label}`}
      className="lp-parent"
      data-testid="run-parent-link"
      {...marks}
      tabIndex={0}
      onClick={(e) => {
        e.stopPropagation();
        goToParent();
      }}
      onKeyDown={(e) => {
        if (e.key !== "Enter" && e.key !== " ") return;
        e.preventDefault();
        e.stopPropagation();
        goToParent();
      }}
    >
      <CornerLeftUp size={11} strokeWidth={1.75} aria-hidden />
      <span>{parent.name || parent.run_id.slice(0, 20)}</span>
    </span>
  );
}
