import { Search } from "lucide-react";
import { IconButton } from "../../../shared/ui/kit";
import { useRunSearchStore } from "../../store/runSearchStore";

/** The loupe of the Run's canvas toolbar: opens and closes the search over
 *  the Run's transcripts (⌘⇧F). */
export default function RunSearchToggle({ runId }: { runId: string }) {
  const open = useRunSearchStore((s) => s.runId === runId);
  const toggle = useRunSearchStore((s) => s.toggle);
  return (
    <IconButton
      icon={Search}
      label="Search the Run's transcripts"
      kbd="⌘⇧F"
      data-testid="toolbar-search"
      pressed={open}
      onClick={() => toggle(runId)}
    />
  );
}
