import { Plus, GitMerge, Info, Undo2, Redo2, SquareTerminal, Box, StickyNote, FilePlus } from "lucide-react";
import type { NodeType } from "../types";
import type { LibraryEntry } from "../api";
import { Tooltip } from "./ui/tooltip";
import LibraryDropdown from "./LibraryDropdown";
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
} from "./ui/dropdown-menu";
import { useEditStore } from "../stores/editStore";

interface Props {
  onAddNode: (type: NodeType) => void;
  onAddNote: () => void;
  // #345: open the "Add node from YAML…" modal (paste/upload a node definition).
  onAddNodeFromYaml: () => void;
  libraryEntries: LibraryEntry[];
  onLibraryDelete: (name: string) => void;
  getDropPosition?: () => { x: number; y: number };
  infoOpen?: boolean;
  onToggleInfo?: () => void;
  // #315: an archived run's canvas is read-only — hide every editing control
  // (add node/note, library insert, merge/script, undo/redo). Only the
  // Pipeline-info button survives so the archived pipeline stays inspectable.
  readOnly?: boolean;
}

export default function EditToolbar({ onAddNode, onAddNote, onAddNodeFromYaml, libraryEntries, onLibraryDelete, getDropPosition, infoOpen, onToggleInfo, readOnly = false }: Props) {
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
      {/* #315: every editing affordance is suppressed on a read-only archived
          canvas. Only the Pipeline-info button (below) survives. */}
      {!readOnly && (
        <>
          {/* #307: the `+` is now a dropdown — create a Node (current behaviour)
              or a canvas Note. The trigger keeps `data-testid="toolbar-add"`;
              the sibling merge/script buttons are unchanged. */}
          <DropdownMenu>
            <DropdownMenuTrigger
              data-testid="toolbar-add"
              className="grid h-7 w-7 cursor-pointer place-items-center rounded text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg active:bg-acc active:text-bg-0 data-[popup-open]:bg-bg-4 data-[popup-open]:text-fg"
              aria-label="Add"
            >
              <Plus size={14} />
            </DropdownMenuTrigger>
            <DropdownMenuContent
              className="min-w-[160px] rounded-md border border-line-strong bg-bg-3 p-1 shadow-lg"
              side="bottom"
              align="start"
            >
              <DropdownMenuItem
                data-testid="add-menu-node"
                className="flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
                style={{ fontSize: "11.5px" }}
                onClick={() => onAddNode("code-mutating")}
              >
                <Box size={13} className="shrink-0 text-fg-4" />
                <span>Node</span>
              </DropdownMenuItem>
              <DropdownMenuItem
                data-testid="add-menu-note"
                className="flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
                style={{ fontSize: "11.5px" }}
                onClick={() => onAddNote()}
              >
                <StickyNote size={13} className="shrink-0 text-fg-4" />
                <span>Note</span>
              </DropdownMenuItem>
              {/* #345: 4th way to create a node — from a pasted/uploaded YAML
                  definition. `FilePlus`, NOT `FileUp` (which belongs to the
                  foreign-workflow import). */}
              <DropdownMenuItem
                data-testid="add-menu-node-from-yaml"
                className="flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
                style={{ fontSize: "11.5px" }}
                onClick={() => onAddNodeFromYaml()}
              >
                <FilePlus size={13} className="shrink-0 text-fg-4" />
                <span>Add node from YAML…</span>
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>

          <span className="mx-0.5 h-4 w-px bg-line" />

          <LibraryDropdown entries={libraryEntries} onDelete={onLibraryDelete} getDropPosition={getDropPosition} onAddNodeFromYaml={onAddNodeFromYaml} />

          {/* No ForEach add-button: a fan-out is a `collection` loop region
              (#151), created by selecting the member(s) and fanning out over a
              list field, not by adding a node. Mirrors the Loop button removal
              (#171). */}

          <Tooltip content="Merge node">
            <button
              data-testid="toolbar-merge"
              onClick={() => onAddNode("merge")}
              className="grid h-7 w-7 cursor-pointer place-items-center rounded text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg active:bg-acc active:text-bg-0"
            >
              <GitMerge size={14} />
            </button>
          </Tooltip>

          <Tooltip content="Script node (deterministic bash)">
            <button
              data-testid="toolbar-script"
              onClick={() => onAddNode("script")}
              className="grid h-7 w-7 cursor-pointer place-items-center rounded text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg active:bg-acc active:text-bg-0"
            >
              <SquareTerminal size={14} />
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
        </>
      )}

      {onToggleInfo && (
        <>
          {/* #315: no leading separator when the info button is the sole control. */}
          {!readOnly && <span className="mx-0.5 h-4 w-px bg-line" />}

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
