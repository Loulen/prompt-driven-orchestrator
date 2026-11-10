import { ExternalLink, Pencil } from "lucide-react";
import { IconButton, Tooltip } from "../../shared/ui/kit";
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
 * source pipeline »; while editing, the pressed pencil is « Finish editing ».
 */
export default function RunEditControl({ runEdit }: { runEdit: RunEdit }) {
  if (runEdit.editing) {
    // While editing, the pencil stays pressed: a click is « Finish editing ».
    return (
      <IconButton
        icon={Pencil}
        label="Finish editing"
        pressed
        data-testid="toolbar-finish-editing"
        onClick={runEdit.onFinishEditing}
      />
    );
  }
  return (
    <DropdownMenu>
      <Tooltip label="Edit">
        <DropdownMenuTrigger data-testid="toolbar-edit" className="d-icon" aria-label="Edit">
          <Pencil size={14} strokeWidth={1.5} aria-hidden />
        </DropdownMenuTrigger>
      </Tooltip>
      <DropdownMenuContent className="cv-menu" side="bottom" align="start">
        <DropdownMenuItem data-testid="toolbar-edit-for-run" className="cv-menu__item" onClick={runEdit.onEditForRun}>
          <Pencil size={14} strokeWidth={1.5} aria-hidden />
          Edit for this run
        </DropdownMenuItem>
        <DropdownMenuItem data-testid="toolbar-edit-source" className="cv-menu__item" onClick={runEdit.onEditSource}>
          <ExternalLink size={14} strokeWidth={1.5} aria-hidden />
          Edit source pipeline
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
