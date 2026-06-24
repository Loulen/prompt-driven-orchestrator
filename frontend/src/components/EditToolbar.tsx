import { Plus, GitMerge, Info, Undo2, Redo2 } from "lucide-react";
import type { NodeType } from "../types";
import type { LibraryEntry } from "../api";
import { Tooltip } from "./ui/tooltip";
import LibraryDropdown from "./LibraryDropdown";
import { useEditStore } from "../stores/editStore";

interface Props {
  onAddNode: (type: NodeType) => void;
  libraryEntries: LibraryEntry[];
  onLibraryDelete: (name: string) => void;
  getDropPosition?: () => { x: number; y: number };
  infoOpen?: boolean;
  onToggleInfo?: () => void;
}

export default function EditToolbar({ onAddNode, libraryEntries, onLibraryDelete, getDropPosition, infoOpen, onToggleInfo }: Props) {
  // Read undo/redo straight from the store (ADR-0014 / #226): they have no
  // component-local dependency, unlike the prop-drilled add/merge callbacks, so
  // the point-of-use selector idiom is the right fit. `canUndo`/`canRedo` are
  // derived (reactive) rather than stored — no duplicated state to keep in sync.
  const undo = useEditStore((s) => s.undo);
  const redo = useEditStore((s) => s.redo);
  const canUndo = useEditStore((s) => {
    const t = s.activeTabId;
    return t != null && (s.history[t]?.past.length ?? 0) > 0;
  });
  const canRedo = useEditStore((s) => {
    const t = s.activeTabId;
    return t != null && (s.history[t]?.future.length ?? 0) > 0;
  });

  return (
    <div
      className="absolute left-3 top-3 z-10 flex items-center gap-0.5 rounded-md border border-line bg-bg-2/90 p-1 backdrop-blur-sm shadow-lg"
      data-testid="edit-toolbar"
    >
      <Tooltip content="New node · N">
        <button
          data-testid="toolbar-add"
          onClick={() => onAddNode("code-mutating")}
          className="grid h-7 w-7 cursor-pointer place-items-center rounded text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg active:bg-acc active:text-bg-0"
        >
          <Plus size={14} />
        </button>
      </Tooltip>

      <span className="mx-0.5 h-4 w-px bg-line" />

      <LibraryDropdown entries={libraryEntries} onDelete={onLibraryDelete} getDropPosition={getDropPosition} />

      {/* No ForEach add-button: a fan-out is a `collection` loop region (#151),
          created by selecting the member(s) and fanning out over a list field,
          not by adding a node. Mirrors the Loop button removal (#171). */}

      <Tooltip content="Merge node">
        <button
          data-testid="toolbar-merge"
          onClick={() => onAddNode("merge")}
          className="grid h-7 w-7 cursor-pointer place-items-center rounded text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg active:bg-acc active:text-bg-0"
        >
          <GitMerge size={14} />
        </button>
      </Tooltip>

      <span className="mx-0.5 h-4 w-px bg-line" />

      <Tooltip content="Undo · Ctrl+Z">
        <button
          data-testid="toolbar-undo"
          onClick={() => undo()}
          disabled={!canUndo}
          className="grid h-7 w-7 place-items-center rounded text-fg-3 transition-colors enabled:cursor-pointer hover:bg-bg-4 hover:text-fg active:bg-acc active:text-bg-0 disabled:cursor-not-allowed disabled:text-fg-5 disabled:hover:bg-transparent disabled:hover:text-fg-5"
        >
          <Undo2 size={14} />
        </button>
      </Tooltip>

      <Tooltip content="Redo · Ctrl+Y">
        <button
          data-testid="toolbar-redo"
          onClick={() => redo()}
          disabled={!canRedo}
          className="grid h-7 w-7 place-items-center rounded text-fg-3 transition-colors enabled:cursor-pointer hover:bg-bg-4 hover:text-fg active:bg-acc active:text-bg-0 disabled:cursor-not-allowed disabled:text-fg-5 disabled:hover:bg-transparent disabled:hover:text-fg-5"
        >
          <Redo2 size={14} />
        </button>
      </Tooltip>

      {onToggleInfo && (
        <>
          <span className="mx-0.5 h-4 w-px bg-line" />

          <Tooltip content="Pipeline info">
            <button
              data-testid="toolbar-info"
              onClick={onToggleInfo}
              className={`grid h-7 w-7 cursor-pointer place-items-center rounded transition-colors ${
                infoOpen
                  ? "bg-acc text-bg-0"
                  : "text-fg-3 hover:bg-bg-4 hover:text-fg active:bg-acc active:text-bg-0"
              }`}
            >
              <Info size={14} />
            </button>
          </Tooltip>
        </>
      )}
    </div>
  );
}
