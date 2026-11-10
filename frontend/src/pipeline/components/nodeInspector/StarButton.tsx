import { Star } from "lucide-react";
import { Menu } from "../../../shared/ui/kit";
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
  function librarySpec() {
    return {
      name: node.name ?? "",
      type: node.type,
      inputs: node.inputs.map((p) => ({
        name: p.name,
        ...(p.port_type && p.port_type !== "markdown" ? { port_type: p.port_type } : {}),
        ...(p.frontmatter ? { frontmatter: p.frontmatter } : {}),
        ...(p.when ? { when: p.when } : {}),
      })),
      outputs: node.outputs.map((p) => ({
        name: p.name,
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
      // ADR-0086: the « on demand » option is starred with the node.
      on_demand: node.on_demand ?? false,
      // ADR-0089: the live artifacts are starred with the node.
      live_artifacts: node.live_artifacts ?? [],
      free_live_artifacts: node.free_live_artifacts ?? false,
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
    }
  }

  async function handleUpdateLibrary() {
    try {
      await saveToLibrary(librarySpec());
      onLibraryChanged();
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
        inputs: result.spec.inputs.map((p) => libraryPortToPortDef(p)),
        outputs: result.spec.outputs.map((p) => libraryPortToPortDef(p)),
        interactive: result.spec.interactive,
        // #296/#345: reset the per-node model from the library entry too.
        model: result.spec.model ?? null,
        // #424: and the effort level.
        effort: result.spec.effort ?? null,
        // #655/ADR-0060: and the workspace — resetting to the library must put
        // the node back where the entry says it works, not where its type would.
        isolated_worktree: result.spec.isolated_worktree ?? null,
        on_demand: result.spec.on_demand ?? false,
        live_artifacts: result.spec.live_artifacts ?? [],
        free_live_artifacts: result.spec.free_live_artifacts ?? false,
      });
      updatePromptFn(result.prompt);
    } catch {
      // ignore
    }
  }

  async function handleRemoveFromLibrary() {
    try {
      await deleteFromLibrary(node.name ?? "");
      onLibraryChanged();
    } catch {
      // ignore
    }
  }

  const isFilled = syncState !== "outline";
  const tooltip = STAR_TOOLTIPS[syncState];
  const star = (onClick: () => void, open = false) => (
    <button
      type="button"
      onClick={onClick}
      className="d-icon"
      title={tooltip}
      aria-label={tooltip}
      aria-expanded={syncState === "outline" ? undefined : open}
      data-state={syncState}
    >
      <Star size={14} strokeWidth={1.5} aria-hidden className={isFilled ? "ei-star is-on" : "ei-star"} />
      {syncState === "diverged" && <span className="ei-star__dot" aria-hidden />}
    </button>
  );

  if (syncState === "outline") return star(() => void handleStarClick());
  return (
    <Menu
      label="Library"
      align="end"
      trigger={({ open, toggle }) => star(toggle, open)}
      entries={
        syncState === "synced"
          ? [{ label: "Remove from library", onSelect: () => void handleRemoveFromLibrary() }]
          : [
              { label: "Update library entry", onSelect: () => void handleUpdateLibrary() },
              { label: "Reset from library", onSelect: () => void handleResetFromLibrary() },
              { kind: "separator" as const },
              { label: "Remove from library", danger: true, onSelect: () => void handleRemoveFromLibrary() },
            ]
      }
    />
  );
}
