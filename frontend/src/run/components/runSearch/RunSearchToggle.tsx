import { Search } from "lucide-react";
import { Tooltip } from "../../../shared/ui/tooltip";
import { useRunSearchStore } from "../../store/runSearchStore";

/** The loupe of the Run's canvas toolbar: opens and closes the search over
 *  the Run's transcripts (⌘⇧F). */
export default function RunSearchToggle({ runId }: { runId: string }) {
  const open = useRunSearchStore((s) => s.runId === runId);
  const toggle = useRunSearchStore((s) => s.toggle);
  return (
    <Tooltip content="Search the Run's transcripts · ⌘⇧F">
      <button
        data-testid="toolbar-search"
        aria-label="Search the Run's transcripts"
        aria-pressed={open}
        onClick={() => toggle(runId)}
        className={`grid h-7 w-7 cursor-pointer place-items-center rounded transition-colors ${
          open ? "bg-surface-2 text-text" : "text-text-3 hover:bg-hover hover:text-text active:bg-primary active:text-surface-0"
        }`}
      >
        <Search size={14} />
      </button>
    </Tooltip>
  );
}
