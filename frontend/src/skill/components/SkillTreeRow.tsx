import { useEffect, useRef, useState, type ReactNode } from "react";
import { ChevronDown, ChevronRight, FileText, Folder, FolderOpen } from "lucide-react";
import type { TreeNodeRef, TreeRow } from "../model/skillTree";

/**
 * One row of the bank's tree: a folder (chevron, count, Source dot) or a skill,
 * its inline rename, its drag handle and drop target, and its actions.
 */
export default function TreeRowView({
  row,
  selected,
  renaming,
  renameError,
  onRenameCommit,
  onRenameCancel,
  onSelect,
  onToggle,
  onDoubleClick,
  actions,
  leading,
  marked = false,
  draggable,
  isDragging,
  isDropTarget,
  onDragStart,
  onDragEnd,
  onDragOver,
  onDragLeave,
  onDrop,
}: {
  row: TreeRow;
  selected: boolean;
  renaming: boolean;
  renameError: string | null;
  onRenameCommit?: (ref: TreeNodeRef, name: string) => void | Promise<void>;
  onRenameCancel?: () => void;
  onSelect: () => void;
  onToggle: () => void;
  onDoubleClick: () => void;
  actions?: ReactNode;
  leading?: ReactNode;
  /** Part of the tree's multiple selection: tinted like a selected row. */
  marked?: boolean;
  draggable: boolean;
  isDragging: boolean;
  isDropTarget: boolean;
  onDragStart: (event: React.DragEvent) => void;
  onDragEnd: () => void;
  onDragOver?: (event: React.DragEvent) => void;
  onDragLeave?: () => void;
  onDrop?: (event: React.DragEvent) => void;
}) {
  const isFolder = row.ref.kind === "folder";
  const name = isFolder ? row.folder?.name ?? "" : row.skill?.name ?? "";
  const indent = 8 + row.depth * 18;

  return (
    <div
      role="treeitem"
      aria-selected={selected}
      aria-expanded={isFolder ? row.expanded : undefined}
      data-testid={`tree-${row.ref.kind}-${row.ref.id}`}
      data-selected={selected || undefined}
      data-marked={marked || undefined}
      data-drop-target={isDropTarget || undefined}
      className={`group relative flex flex-col rounded-md ${
        isDropTarget
          ? "bg-primary/15 outline outline-1 outline-primary"
          : selected
            ? "bg-primary/10 outline outline-1 outline-primary/40"
            : marked
              ? "bg-primary/10 hover:bg-primary/15"
              : "hover:bg-active"
      } ${isDragging ? "opacity-40" : ""}`}
      onDragOver={onDragOver}
      onDragLeave={onDragLeave}
      onDrop={onDrop}
      onClick={onSelect}
      onDoubleClick={(event) => {
        event.stopPropagation();
        onDoubleClick();
      }}
    >
      <div
        className="flex min-h-[30px] items-center gap-1.5 pr-1"
        style={{ paddingLeft: indent }}
        draggable={draggable && !renaming}
        onDragStart={draggable ? onDragStart : undefined}
        onDragEnd={draggable ? onDragEnd : undefined}
      >
        {leading}
        {isFolder ? (
          <button
            type="button"
            aria-label={row.expanded ? `Collapse ${name}` : `Expand ${name}`}
            onClick={(event) => {
              event.stopPropagation();
              onToggle();
            }}
            className="grid h-4 w-4 shrink-0 place-items-center rounded text-text-3 hover:text-text"
          >
            {row.expanded ? <ChevronDown size={11} /> : <ChevronRight size={11} />}
          </button>
        ) : (
          <span className="h-4 w-4 shrink-0" />
        )}
        {isFolder ? (
          row.expanded ? (
            <FolderOpen size={13} className="shrink-0 text-st-await" />
          ) : (
            <Folder size={13} className="shrink-0 text-st-await" />
          )
        ) : (
          <FileText size={13} className="shrink-0 text-text-3" />
        )}
        {renaming ? (
          <RenameInput
            initial={name}
            onCommit={(value) => onRenameCommit?.(row.ref, value)}
            onCancel={() => onRenameCancel?.()}
          />
        ) : (
          <span
            className={`min-w-0 flex-1 truncate ${isFolder ? "font-medium text-text" : "text-text-2"} text-sm`}
          >
            {name}
          </span>
        )}
        {isFolder && !renaming && (
          <span className="flex shrink-0 items-center gap-1.5 group-hover:hidden">
            {row.folder?.source && (
              <span
                className="h-2 w-2 rounded-full border-[1.5px] border-primary"
                title={`Imported from ${row.folder.source.url}`}
                aria-label="Source folder"
                data-testid={`tree-source-${row.ref.id}`}
              />
            )}
            <span className="font-mono text-text-3 text-2xs">
              {row.count}
            </span>
          </span>
        )}
        {!renaming && actions && (
          <span
            className={`shrink-0 items-center gap-0.5 ${selected ? "flex" : "hidden group-hover:flex group-focus-within:flex"}`}
            onClick={(event) => event.stopPropagation()}
            onDoubleClick={(event) => event.stopPropagation()}
          >
            {actions}
          </span>
        )}
      </div>
      {renaming && (
        <div className="pb-1 text-text-3 text-2xs" style={{ paddingLeft: indent + 22 }}>
          {renameError ? (
            <span className="text-st-failed" data-testid="rename-error">
              {renameError}
            </span>
          ) : (
            "Enter to save · Esc to cancel"
          )}
        </div>
      )}
    </div>
  );
}

function RenameInput({
  initial,
  onCommit,
  onCancel,
}: {
  initial: string;
  onCommit: (value: string) => void | Promise<void>;
  onCancel: () => void;
}) {
  const [value, setValue] = useState(initial);
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    ref.current?.focus();
    ref.current?.select();
  }, []);
  return (
    <input
      ref={ref}
      value={value}
      aria-label="New name"
      data-testid="rename-input"
      onChange={(event) => setValue(event.target.value)}
      onClick={(event) => event.stopPropagation()}
      onKeyDown={(event) => {
        event.stopPropagation();
        if (event.key === "Enter") {
          event.preventDefault();
          const trimmed = value.trim();
          if (trimmed === "" || trimmed === initial) onCancel();
          else void onCommit(trimmed);
        } else if (event.key === "Escape") {
          event.preventDefault();
          onCancel();
        }
      }}
      onBlur={() => {
        // Blur commits like Enter (a click elsewhere should not lose the edit),
        // except when nothing changed.
        const trimmed = value.trim();
        if (trimmed === "" || trimmed === initial) onCancel();
        else void onCommit(trimmed);
      }}
      className="min-w-0 flex-1 rounded border border-primary bg-surface-0 px-1.5 py-0.5 text-text outline-none text-sm"
    />
  );
}
