import { Plus, Repeat, GitBranch, GitMerge, Info } from "lucide-react";
import { ForEachIcon } from "./ForEachNode";
import type { NodeType } from "../types";
import type { LibraryEntry } from "../api";
import { Tooltip } from "./ui/tooltip";
import LibraryDropdown from "./LibraryDropdown";

interface Props {
  onAddNode: (type: NodeType) => void;
  libraryEntries: LibraryEntry[];
  onLibraryDelete: (name: string) => void;
  getDropPosition?: () => { x: number; y: number };
  infoOpen?: boolean;
  onToggleInfo?: () => void;
}

export default function EditToolbar({ onAddNode, libraryEntries, onLibraryDelete, getDropPosition, infoOpen, onToggleInfo }: Props) {
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

      <Tooltip content="Loop node">
        <button
          data-testid="toolbar-loop"
          onClick={() => onAddNode("loop")}
          className="grid h-7 w-7 cursor-pointer place-items-center rounded text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg active:bg-acc active:text-bg-0"
        >
          <Repeat size={14} />
        </button>
      </Tooltip>

      <Tooltip content="ForEach node">
        <button
          data-testid="toolbar-foreach"
          onClick={() => onAddNode("for-each")}
          className="grid h-7 w-7 cursor-pointer place-items-center rounded text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg active:bg-acc active:text-bg-0"
        >
          <ForEachIcon />
        </button>
      </Tooltip>

      <Tooltip content="Switch node">
        <button
          data-testid="toolbar-switch"
          onClick={() => onAddNode("switch")}
          className="grid h-7 w-7 cursor-pointer place-items-center rounded text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg active:bg-acc active:text-bg-0"
        >
          <GitBranch size={14} />
        </button>
      </Tooltip>

      <Tooltip content="Merge node">
        <button
          data-testid="toolbar-merge"
          onClick={() => onAddNode("merge")}
          className="grid h-7 w-7 cursor-pointer place-items-center rounded text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg active:bg-acc active:text-bg-0"
        >
          <GitMerge size={14} />
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
