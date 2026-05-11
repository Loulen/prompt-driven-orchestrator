import { useEffect, useRef, useState } from "react";
import { Star } from "lucide-react";
import type { PipelineDef } from "../types";
import type { LibraryPipelineEntry, LibraryPipelineScope } from "../api";
import { saveLibraryPipeline, deleteLibraryPipeline } from "../api";
import { serializePipeline, useEditStore } from "../stores/editStore";
import type { PipelineLibrarySyncState } from "../hooks/useLibraryPipelines";
import { Tooltip } from "./ui/tooltip";

const TOOLTIPS: Record<PipelineLibrarySyncState, string> = {
  outline: "Save pipeline to library",
  synced: "In your library — synced",
  diverged: "In your library — out of sync",
};

interface Props {
  tabId: string;
  pipeline: PipelineDef;
  syncState: PipelineLibrarySyncState;
  libraryEntry: LibraryPipelineEntry | null;
  onLibraryChanged: () => void;
}

// When the user clicks the outline star for the very first time, default the
// new library entry to repo scope. The user works inside a concrete repo and
// almost always wants the template to follow it; user-scope is the explicit
// override available from the Pipeline Inspector.
const DEFAULT_NEW_STAR_SCOPE: LibraryPipelineScope = "repo";

export default function PipelineStar({
  tabId,
  pipeline,
  syncState,
  libraryEntry,
  onLibraryChanged,
}: Props) {
  const [popoverOpen, setPopoverOpen] = useState(false);
  const popoverRef = useRef<HTMLDivElement>(null);
  const reloadFromLibrary = useEditStore((s) => s.reloadFromLibrary);
  const setLibraryBinding = useEditStore((s) => s.setLibraryBinding);
  const tab = useEditStore((s) => s.openTabs.find((t) => t.id === tabId));
  const tabPrompts = tab?.prompts;
  const libraryId = tab?.libraryId ?? libraryEntry?.id ?? null;
  const libraryScope = tab?.libraryScope ?? libraryEntry?.scope ?? null;

  useEffect(() => {
    if (!popoverOpen) return;
    function handleClickOutside(e: MouseEvent) {
      if (popoverRef.current && !popoverRef.current.contains(e.target as Node)) {
        setPopoverOpen(false);
      }
    }
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [popoverOpen]);

  async function handleSaveToLibrary() {
    try {
      const yaml = serializePipeline(pipeline);
      const scope: LibraryPipelineScope = libraryScope ?? DEFAULT_NEW_STAR_SCOPE;
      const result = await saveLibraryPipeline(
        pipeline.name,
        yaml,
        tabPrompts ?? {},
        {
          ...(libraryId ? { id: libraryId } : {}),
          scope,
        },
      );
      // Lock the resulting id+scope to this tab so future renames don't fall
      // back to slug(name) and orphan the entry we just wrote.
      setLibraryBinding(tabId, result.id, result.scope);
      onLibraryChanged();
    } catch {
      // ignore
    }
  }

  async function handleStarClick() {
    if (syncState === "outline") {
      await handleSaveToLibrary();
    } else {
      setPopoverOpen((v) => !v);
    }
  }

  async function handleUpdateLibrary() {
    await handleSaveToLibrary();
    setPopoverOpen(false);
  }

  async function handleResetFromLibrary() {
    if (!libraryEntry) return;
    try {
      await reloadFromLibrary(tabId, libraryEntry.yaml);
      setPopoverOpen(false);
    } catch {
      // ignore
    }
  }

  async function handleRemoveFromLibrary() {
    if (!libraryEntry && !libraryId) return;
    const idToDelete = libraryEntry?.id ?? libraryId;
    if (!idToDelete) return;
    try {
      await deleteLibraryPipeline(idToDelete);
      setLibraryBinding(tabId, null, null);
      onLibraryChanged();
      setPopoverOpen(false);
    } catch {
      // ignore
    }
  }

  const isFilled = syncState !== "outline";

  return (
    <div className="relative" ref={popoverRef}>
      <Tooltip content={TOOLTIPS[syncState]}>
        <button
          onClick={handleStarClick}
          className="grid h-7 w-7 place-items-center rounded-md border border-line bg-bg-2 shadow-sm transition-colors hover:bg-bg-3"
          title={TOOLTIPS[syncState]}
          data-testid="pipeline-star"
          data-sync-state={syncState}
        >
          <span className="relative">
            <Star
              size={14}
              className={isFilled ? "fill-acc text-acc" : "fill-none text-fg-4"}
            />
            {syncState === "diverged" && (
              <span
                className="absolute -bottom-0.5 -right-0.5 h-1.5 w-1.5 rounded-full bg-st-blocked"
                data-testid="pipeline-star-diverged-dot"
              />
            )}
          </span>
        </button>
      </Tooltip>

      {popoverOpen && (
        <div
          className="absolute right-0 top-full z-50 mt-1 w-[200px] rounded-lg border border-line bg-bg-4 py-1 shadow-lg"
          style={{ fontSize: "11px" }}
          data-testid="pipeline-star-popover"
        >
          {syncState === "synced" && (
            <button
              onClick={handleRemoveFromLibrary}
              className="w-full cursor-pointer px-3 py-1.5 text-left text-fg-3 transition-colors hover:bg-bg-3 hover:text-fg"
            >
              Remove from library
            </button>
          )}
          {syncState === "diverged" && (
            <>
              <button
                onClick={handleUpdateLibrary}
                className="w-full cursor-pointer px-3 py-1.5 text-left text-fg-3 transition-colors hover:bg-bg-3 hover:text-fg"
              >
                Update library entry
              </button>
              <button
                onClick={handleResetFromLibrary}
                className="w-full cursor-pointer px-3 py-1.5 text-left text-fg-3 transition-colors hover:bg-bg-3 hover:text-fg"
              >
                Reset from library
              </button>
              <button
                onClick={handleRemoveFromLibrary}
                className="w-full cursor-pointer px-3 py-1.5 text-left text-fg-3 transition-colors hover:bg-bg-3 hover:text-fg"
              >
                Remove from library
              </button>
            </>
          )}
        </div>
      )}
    </div>
  );
}
