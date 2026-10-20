import { History, Info, RotateCcw } from "lucide-react";
import type { Actor } from "../../shared/generated/Actor";
import { actorLabel } from "../../shared/lib/actor";
import { useActorDisplay } from "../../shared/actors/store";
import { toast } from "../../shared/ui/toast";
import { savedOn } from "../model/versions";
import type { VersionTabState } from "../store/editState";
import { useEditStore } from "../store/editStore";

function SavedBy({ actor }: { actor: Actor | null }) {
  const display = useActorDisplay(actor);
  if (!actor) return <>written to the file</>;
  return <>saved by {display?.name ?? actorLabel(actor)}</>;
}

/**
 * Above the canvas of a version shown read only (ADR-0088): which version,
 * who saved it and when, and « Restore into draft ».
 */
export default function VersionBanner({
  version,
  infoOpen = false,
  onToggleInfo,
}: {
  version: VersionTabState;
  /** Pipeline info, where the history is: the toolbar's `(i)`. */
  infoOpen?: boolean;
  onToggleInfo?: () => void;
}) {
  const restoreVersion = useEditStore((s) => s.restoreVersion);
  const restore = async () => {
    try {
      await restoreVersion(version.pipelineId, version.number);
      toast(`v${version.number} restored into the draft. Undo takes it back.`);
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e), "error");
    }
  };
  return (
    <div
      className="absolute left-3 right-3 top-3 z-10 flex items-center gap-2 rounded-md border border-line bg-bg-2/95 px-3 py-1.5 text-fg-3 shadow-lg backdrop-blur-sm"
      style={{ fontSize: "11px" }}
      data-testid="version-banner"
    >
      {onToggleInfo && (
        <button
          onClick={onToggleInfo}
          aria-label="Pipeline info"
          aria-pressed={infoOpen}
          className={`grid h-5 w-5 shrink-0 cursor-pointer place-items-center rounded transition-colors hover:bg-bg-4 hover:text-fg ${
            infoOpen ? "bg-bg-4 text-fg" : "text-fg-3"
          }`}
          data-testid="version-banner-info"
        >
          <Info size={12} />
        </button>
      )}
      <History size={12} className="shrink-0 text-fg-4" />
      <span className="min-w-0 truncate">
        Viewing <span className="font-mono text-fg">v{version.number}</span>
        {" · "}
        <span className="font-mono text-fg">{version.title}</span>, read-only · <SavedBy actor={version.author} /> on{" "}
        {savedOn(version.savedAt)}
      </span>
      <button
        onClick={() => void restore()}
        className="ml-auto flex shrink-0 cursor-pointer items-center gap-1 rounded-md bg-acc px-2 py-0.5 font-medium text-on-acc transition-colors hover:bg-acc-dim"
        data-testid="version-banner-restore"
      >
        <RotateCcw size={11} />
        Restore into draft
      </button>
    </div>
  );
}
