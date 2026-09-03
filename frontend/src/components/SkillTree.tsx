import { useEffect, useRef, useState, type ReactNode } from "react";
import { ChevronDown, ChevronRight, FileText, Folder, FolderOpen } from "lucide-react";
import type { Skill, SkillFolder } from "../types";
import { buildRows, sameRef, type TreeNodeRef, type TreeRow } from "../lib/skillTree";

export interface SkillTreeProps {
  folders: SkillFolder[];
  skills: Skill[];
  selected: TreeNodeRef | null;
  onSelect: (ref: TreeNodeRef) => void;
  expanded: ReadonlySet<string>;
  onToggle: (folderId: string) => void;
  filter?: string;
  /** Row currently in inline rename, if any. */
  renaming?: TreeNodeRef | null;
  renameError?: string | null;
  onRenameCommit?: (ref: TreeNodeRef, name: string) => void | Promise<void>;
  onRenameCancel?: () => void;
  /** Called when a skill is dropped on a folder (`null` = the root area). */
  onDropSkill?: (skillId: string, folderId: string | null) => void;
  /** Hover / focus actions rendered at the right of a row (pencil, kebab…). */
  renderActions?: (row: TreeRow) => ReactNode;
  /** Keyboard verbs on the selected row. */
  onRequestRename?: (ref: TreeNodeRef) => void;
  onRequestDelete?: (ref: TreeNodeRef) => void;
  onOpen?: (ref: TreeNodeRef) => void;
  /** Rendered when `rows` is empty (empty bank or no filter match). */
  emptyState?: ReactNode;
  /** Optional row decoration (e.g. checkboxes for the future selector). */
  renderLeading?: (row: TreeRow) => ReactNode;
  draggable?: boolean;
  className?: string;
}

/**
 * The bank's tree (#668): folders first, then skills, at every level; counts on
 * folders; inline rename; native HTML5 drag of a skill onto a folder (or onto
 * the empty area below the rows, which is the root). Keyboard: ↑↓ move, →← fold
 * a folder, F2 rename, Delete asks for deletion, Enter opens.
 *
 * Deliberately generic — the future tier selector renders the same rows with a
 * `renderLeading` checkbox instead of drag (design note in #668).
 */
export default function SkillTree({
  folders,
  skills,
  selected,
  onSelect,
  expanded,
  onToggle,
  filter = "",
  renaming = null,
  renameError = null,
  onRenameCommit,
  onRenameCancel,
  onDropSkill,
  renderActions,
  onRequestRename,
  onRequestDelete,
  onOpen,
  emptyState,
  renderLeading,
  draggable = true,
  className = "",
}: SkillTreeProps) {
  const rows = buildRows(folders, skills, expanded, filter);
  const [dropTarget, setDropTarget] = useState<string | "root" | null>(null);
  const [dragging, setDragging] = useState<string | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  const onKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (renaming) return; // the input owns the keys
    if ((event.target as HTMLElement).tagName === "INPUT") return;
    const index = rows.findIndex((row) => sameRef(row.ref, selected));
    const select = (i: number) => {
      const row = rows[Math.max(0, Math.min(rows.length - 1, i))];
      if (row) onSelect(row.ref);
    };
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        select(index < 0 ? 0 : index + 1);
        break;
      case "ArrowUp":
        event.preventDefault();
        select(index < 0 ? 0 : index - 1);
        break;
      case "ArrowRight": {
        const row = rows[index];
        if (row?.ref.kind === "folder" && !row.expanded) {
          event.preventDefault();
          onToggle(row.ref.id);
        }
        break;
      }
      case "ArrowLeft": {
        const row = rows[index];
        if (row?.ref.kind === "folder" && row.expanded) {
          event.preventDefault();
          onToggle(row.ref.id);
        } else if (row) {
          // Jump to the parent folder.
          const parentId = row.ref.kind === "skill" ? row.skill?.folder_id : row.folder?.parent_id;
          if (parentId) {
            event.preventDefault();
            onSelect({ kind: "folder", id: parentId });
          }
        }
        break;
      }
      case "F2":
        if (selected && onRequestRename) {
          event.preventDefault();
          onRequestRename(selected);
        }
        break;
      case "Delete":
        if (selected && onRequestDelete) {
          event.preventDefault();
          onRequestDelete(selected);
        }
        break;
      case "Enter":
        if (selected && onOpen) {
          event.preventDefault();
          onOpen(selected);
        }
        break;
      default:
        break;
    }
  };

  const dropOnFolder = (event: React.DragEvent, folderId: string | null) => {
    event.preventDefault();
    event.stopPropagation();
    const skillId = event.dataTransfer.getData("text/pdo-skill") || dragging;
    setDropTarget(null);
    setDragging(null);
    if (skillId && onDropSkill) onDropSkill(skillId, folderId);
  };

  const allowDrop = (event: React.DragEvent, target: string | "root") => {
    if (!dragging && !event.dataTransfer.types.includes("text/pdo-skill")) return;
    event.preventDefault();
    event.stopPropagation();
    event.dataTransfer.dropEffect = "move";
    if (dropTarget !== target) setDropTarget(target);
  };

  return (
    <div
      ref={containerRef}
      role="tree"
      tabIndex={0}
      aria-label="Skills"
      data-testid="skill-tree"
      onKeyDown={onKeyDown}
      className={`flex min-h-0 flex-1 flex-col outline-none ${className}`}
    >
      <div className="flex flex-col gap-px px-2 pt-1">
        {rows.map((row) => (
          <TreeRowView
            key={`${row.ref.kind}:${row.ref.id}`}
            row={row}
            selected={sameRef(row.ref, selected)}
            renaming={sameRef(row.ref, renaming)}
            renameError={renameError}
            onRenameCommit={onRenameCommit}
            onRenameCancel={onRenameCancel}
            onSelect={() => onSelect(row.ref)}
            onToggle={() => row.ref.kind === "folder" && onToggle(row.ref.id)}
            onDoubleClick={() => {
              if (onRequestRename) onRequestRename(row.ref);
            }}
            actions={renderActions?.(row)}
            leading={renderLeading?.(row)}
            draggable={draggable && row.ref.kind === "skill"}
            isDragging={dragging === row.ref.id}
            isDropTarget={row.ref.kind === "folder" && dropTarget === row.ref.id}
            onDragStart={(event) => {
              event.dataTransfer.setData("text/pdo-skill", row.ref.id);
              event.dataTransfer.effectAllowed = "move";
              setDragging(row.ref.id);
            }}
            onDragEnd={() => {
              setDragging(null);
              setDropTarget(null);
            }}
            onDragOver={row.ref.kind === "folder" ? (event) => allowDrop(event, row.ref.id) : undefined}
            onDragLeave={row.ref.kind === "folder" ? () => setDropTarget((t) => (t === row.ref.id ? null : t)) : undefined}
            onDrop={row.ref.kind === "folder" ? (event) => dropOnFolder(event, row.ref.id) : undefined}
          />
        ))}
      </div>
      {/* Root drop zone: the space below the rows. */}
      <div
        className={`min-h-[48px] flex-1 rounded-md transition-colors ${
          dropTarget === "root" ? "bg-acc/10 outline-dashed outline-1 outline-acc/50" : ""
        }`}
        data-testid="skill-tree-root-drop"
        onDragOver={(event) => allowDrop(event, "root")}
        onDragLeave={() => setDropTarget((t) => (t === "root" ? null : t))}
        onDrop={(event) => dropOnFolder(event, null)}
      >
        {rows.length === 0 && emptyState}
      </div>
    </div>
  );
}

function TreeRowView({
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
      data-drop-target={isDropTarget || undefined}
      className={`group relative flex flex-col rounded-md ${
        isDropTarget
          ? "bg-acc/15 outline outline-1 outline-acc"
          : selected
            ? "bg-acc/10 outline outline-1 outline-acc/40"
            : "hover:bg-bg-5"
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
        {isFolder ? (
          <button
            type="button"
            aria-label={row.expanded ? `Collapse ${name}` : `Expand ${name}`}
            onClick={(event) => {
              event.stopPropagation();
              onToggle();
            }}
            className="grid h-4 w-4 shrink-0 place-items-center rounded text-fg-4 hover:text-fg"
          >
            {row.expanded ? <ChevronDown size={11} /> : <ChevronRight size={11} />}
          </button>
        ) : (
          <span className="h-4 w-4 shrink-0" />
        )}
        {leading}
        {isFolder ? (
          row.expanded ? (
            <FolderOpen size={13} className="shrink-0 text-st-await" />
          ) : (
            <Folder size={13} className="shrink-0 text-st-await" />
          )
        ) : (
          <FileText size={13} className="shrink-0 text-fg-3" />
        )}
        {renaming ? (
          <RenameInput
            initial={name}
            onCommit={(value) => onRenameCommit?.(row.ref, value)}
            onCancel={() => onRenameCancel?.()}
          />
        ) : (
          <span
            className={`min-w-0 flex-1 truncate ${isFolder ? "font-medium text-fg" : "text-fg-2"}`}
            style={{ fontSize: "12px" }}
          >
            {name}
          </span>
        )}
        {isFolder && !renaming && (
          <span className="shrink-0 font-mono text-fg-4 group-hover:hidden" style={{ fontSize: "10px" }}>
            {row.count}
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
        <div className="pb-1 text-fg-4" style={{ paddingLeft: indent + 22, fontSize: "10px" }}>
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
      className="min-w-0 flex-1 rounded border border-acc bg-bg-1 px-1.5 py-0.5 text-fg outline-none"
      style={{ fontSize: "12px" }}
    />
  );
}
