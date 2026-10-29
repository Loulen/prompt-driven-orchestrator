import { useState } from "react";
import { ChevronDown, Save } from "lucide-react";
import { useEditStore } from "../store/editStore";
import type { OpenPipeline } from "../store/editState";
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
} from "../../shared/ui/dropdown-menu";
import OverwriteDefaultPipelineModal from "./OverwriteDefaultPipelineModal";

/**
 * ADR-0085: Save while editing a Run. « Save for this run » writes the Run's
 * snapshot only; the shared pipeline moves through the menu's explicit, warned
 * « Overwrite default pipeline… ».
 */
export default function RunSaveGroup({ tab }: { tab: OpenPipeline & { runId: string } }) {
  const save = useEditStore((s) => s.save);
  const [overwriting, setOverwriting] = useState(false);
  return (
    <div className="flex items-center" data-testid="run-save-group">
      <button
        onClick={() => save(tab.id)}
        disabled={!tab.dirty}
        className="flex cursor-pointer items-center gap-1 rounded-l-md bg-primary px-2 py-0.5 font-medium text-primary-ink transition-colors hover:bg-primary-hi disabled:opacity-40 text-xs"
        data-testid="save-button"
      >
        <Save size={11} />
        Save for this run
      </button>
      <DropdownMenu>
        <DropdownMenuTrigger
          data-testid="save-menu"
          aria-label="More save options"
          className="flex cursor-pointer items-center self-stretch rounded-r-md border-l border-surface-0/30 bg-primary px-1 text-primary-ink transition-colors hover:bg-primary-hi"
        >
          <ChevronDown size={11} />
        </DropdownMenuTrigger>
        <DropdownMenuContent
          className="min-w-[200px] rounded-md border border-line-strong bg-surface-2 p-1 shadow-lg"
          side="bottom"
          align="end"
        >
          <DropdownMenuItem
            data-testid="save-menu-overwrite"
            className="flex cursor-pointer items-center rounded px-2 py-1.5 text-text-2 transition-colors hover:bg-hover text-xs"
            onClick={() => setOverwriting(true)}
          >
            Overwrite default pipeline…
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      {overwriting && (
        <OverwriteDefaultPipelineModal tab={tab} onClose={() => setOverwriting(false)} />
      )}
    </div>
  );
}
