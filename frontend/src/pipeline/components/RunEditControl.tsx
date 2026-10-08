import { Check, ExternalLink, Pencil } from "lucide-react";
import { Tooltip } from "../../shared/ui/tooltip";
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
} from "../../shared/ui/dropdown-menu";

/** The « Edit » control of a non-archived Run's canvas (ADR-0085). */
export interface RunEdit {
  /** `false`: « pilotage », the pencil opens the Edit menu. `true`: « Finish editing ». */
  editing: boolean;
  onEditForRun: () => void;
  onEditSource: () => void;
  onFinishEditing: () => void;
}

/**
 * The one way out of « pilotage »: following a Run never modifies it, editing is
 * an explicit gesture, for this Run only unless the user later overwrites the
 * default pipeline from Save. A pencil opens « Edit for this run » / « Edit
 * source pipeline »; while editing, « Finish editing » takes its place.
 */
export default function RunEditControl({ runEdit }: { runEdit: RunEdit }) {
  if (runEdit.editing) {
    return (
      <Tooltip content="Back to following the run: the canvas locks again">
        <button
          data-testid="toolbar-finish-editing"
          onClick={runEdit.onFinishEditing}
          className="flex h-7 cursor-pointer items-center gap-1 rounded px-2 text-acc transition-colors hover:bg-bg-4 active:bg-acc active:text-bg-0"
          style={{ fontSize: "11px" }}
        >
          <Check size={13} />
          <span>Finish editing</span>
        </button>
      </Tooltip>
    );
  }
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        data-testid="toolbar-edit"
        className="grid h-7 w-7 cursor-pointer place-items-center rounded text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg data-[popup-open]:bg-bg-4 data-[popup-open]:text-fg"
        aria-label="Edit"
        title="Edit"
      >
        <Pencil size={13} />
      </DropdownMenuTrigger>
      <DropdownMenuContent
        className="min-w-[200px] rounded-md border border-line-strong bg-bg-3 p-1 shadow-lg"
        side="bottom"
        align="start"
      >
        <DropdownMenuItem
          data-testid="toolbar-edit-for-run"
          className="flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
          style={{ fontSize: "11.5px" }}
          onClick={runEdit.onEditForRun}
        >
          <Pencil size={12} className="shrink-0 text-fg-4" />
          Edit for this run
        </DropdownMenuItem>
        <DropdownMenuItem
          data-testid="toolbar-edit-source"
          className="flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
          style={{ fontSize: "11.5px" }}
          onClick={runEdit.onEditSource}
        >
          <ExternalLink size={12} className="shrink-0 text-fg-4" />
          Edit source pipeline
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
