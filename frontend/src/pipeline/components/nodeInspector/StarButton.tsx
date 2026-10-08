import { useEffect, useRef, useState } from "react";
import { Star } from "lucide-react";
import type { NodeType } from "../../../shared/generated/NodeType";
import { nodeIsolation } from "../../../lib/nodeIsolation";
import { saveToLibrary, deleteFromLibrary, instantiateFromLibrary } from "../../api/library";
import { libraryPortToPortDef } from "../../model/library";
import type { LibrarySyncState } from "../../../hooks/useLibrary";
import type { EditorNodeDef } from "../../model/pipeline";

const STAR_TOOLTIPS: Record<LibrarySyncState, string> = {
  outline: "Save to library",
  synced: "In your library — synced",
  diverged: "In your library — out of sync",
};
/** The node's library star: save it, or sync or remove its library twin. */
export default function StarButton({
  syncState,
  node,
  prompt,
  onLibraryChanged,
  updateNodeFn,
  updatePromptFn,
}: {
  syncState: LibrarySyncState;
  node: EditorNodeDef;
  prompt: string;
  onLibraryChanged: () => void;
  updateNodeFn: (updates: Partial<EditorNodeDef>) => void;
  updatePromptFn: (content: string) => void;
}) {
  const [popoverOpen, setPopoverOpen] = useState(false);
  const popoverRef = useRef<HTMLDivElement>(null);

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

  function librarySpec() {
    return {
      name: node.name ?? "",
      type: node.type,
      inputs: node.inputs.map((p) => ({
        name: p.name,
        repeated: p.repeated,
        side: p.side,
        ...(p.port_type && p.port_type !== "markdown" ? { port_type: p.port_type } : {}),
        ...(p.frontmatter ? { frontmatter: p.frontmatter } : {}),
        ...(p.when ? { when: p.when } : {}),
      })),
      outputs: node.outputs.map((p) => ({
        name: p.name,
        repeated: p.repeated,
        side: p.side,
        ...(p.port_type && p.port_type !== "markdown" ? { port_type: p.port_type } : {}),
        ...(p.frontmatter ? { frontmatter: p.frontmatter } : {}),
        ...(p.when ? { when: p.when } : {}),
        ...(p.instructions?.trim() ? { instructions: p.instructions } : {}),
      })),
      interactive: node.interactive,
      // #296/#345: persist the per-node model so the library is model-aware and
      // a modelled node stays synced instead of flipping to diverged.
      model: node.model ?? null,
      // #424: same for the effort level. NOTE: `librarySpec()` has NO return-type
      // annotation, so omitting a field here compiles fine — the star would just
      // read `diverged` forever.
      effort: node.effort ?? null,
      // #655/ADR-0060: star where the node works, resolved (not raw) so a node
      // sitting silently on its type's default stars on that default instead of
      // on a silence — a silence would read `diverged` the instant it was saved.
      isolated_worktree: nodeIsolation(node),
      prompt,
    };
  }

  async function handleStarClick() {
    if (syncState === "outline") {
      try {
        await saveToLibrary(librarySpec());
        onLibraryChanged();
      } catch {
        // ignore
      }
    } else {
      setPopoverOpen(!popoverOpen);
    }
  }

  async function handleUpdateLibrary() {
    try {
      await saveToLibrary(librarySpec());
      onLibraryChanged();
      setPopoverOpen(false);
    } catch {
      // ignore
    }
  }

  async function handleResetFromLibrary() {
    try {
      const result = await instantiateFromLibrary(node.name ?? "");
      updateNodeFn({
        name: result.spec.name,
        type: result.spec.type as NodeType,
        inputs: result.spec.inputs.map((p) => libraryPortToPortDef(p, "left")),
        outputs: result.spec.outputs.map((p) => libraryPortToPortDef(p, "right")),
        interactive: result.spec.interactive,
        // #296/#345: reset the per-node model from the library entry too.
        model: result.spec.model ?? null,
        // #424: and the effort level.
        effort: result.spec.effort ?? null,
        // #655/ADR-0060: and the workspace — resetting to the library must put
        // the node back where the entry says it works, not where its type would.
        isolated_worktree: result.spec.isolated_worktree ?? null,
      });
      updatePromptFn(result.prompt);
      setPopoverOpen(false);
    } catch {
      // ignore
    }
  }

  async function handleRemoveFromLibrary() {
    try {
      await deleteFromLibrary(node.name ?? "");
      onLibraryChanged();
      setPopoverOpen(false);
    } catch {
      // ignore
    }
  }

  const isFilled = syncState !== "outline";
  const tooltip = STAR_TOOLTIPS[syncState];

  return (
    <div className="relative" ref={popoverRef}>
      <button
        onClick={handleStarClick}
        className="grid h-6 w-6 place-items-center rounded transition-colors hover:bg-bg-3"
        title={tooltip}
      >
        <span className="relative">
          <Star
            size={14}
            className={
              isFilled ? "fill-acc text-acc" : "fill-none text-fg-4"
            }
          />
          {syncState === "diverged" && (
            <span
              className="absolute -bottom-0.5 -right-0.5 h-1.5 w-1.5 rounded-full bg-st-blocked"
            />
          )}
        </span>
      </button>

      {popoverOpen && (
        <div
          className="absolute right-0 top-full z-50 mt-1 w-[200px] rounded-lg border border-line bg-bg-4 py-1 shadow-lg"
          style={{ fontSize: "11px" }}
        >
          {syncState === "synced" && (
            <button
              onClick={handleRemoveFromLibrary}
              className="w-full px-3 py-1.5 text-left text-fg-3 transition-colors hover:bg-bg-3 hover:text-fg"
            >
              Remove from library
            </button>
          )}
          {syncState === "diverged" && (
            <>
              <button
                onClick={handleUpdateLibrary}
                className="w-full px-3 py-1.5 text-left text-fg-3 transition-colors hover:bg-bg-3 hover:text-fg"
              >
                Update library entry
              </button>
              <button
                onClick={handleResetFromLibrary}
                className="w-full px-3 py-1.5 text-left text-fg-3 transition-colors hover:bg-bg-3 hover:text-fg"
              >
                Reset from library
              </button>
              <button
                onClick={handleRemoveFromLibrary}
                className="w-full px-3 py-1.5 text-left text-fg-3 transition-colors hover:bg-bg-3 hover:text-fg"
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
