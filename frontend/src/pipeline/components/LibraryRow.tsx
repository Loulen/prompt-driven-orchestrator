import { useMemo, type MouseEvent, type ReactNode } from "react";
import { Copy, FolderOpen, Pencil, Trash2 } from "lucide-react";
import SelectControl from "../../shared/ui/SelectControl";
import { ExtensionMenuGroups } from "../../shared/extensions/ExtensionActions";
import type { PipelineEntity } from "../../shared/extensions/contract";
import { DropdownMenuSeparator } from "../../shared/ui/dropdown-menu";
import RowMenu, { ROW_MENU_EXTENSION_ITEM, RowMenuItem } from "../../shared/ui/RowMenu";

interface Props {
  name: string;
  nodeCount: number;
  modified?: string | null;
  /** Highlighted as the open editor tab. Only meaningful on an openable row. */
  selected?: boolean;
  showDuplicate: boolean;
  /**
   * Present ⇒ the row is an openable `<button>`; absent ⇒ a passive `<div>`.
   * That is the real axis between the two library lists: a row backed by a
   * /pipelines entry can be opened in the editor, a library-only row cannot.
   */
  onOpen?: () => void;
  onDuplicate?: () => void;
  /**
   * #774 — present ⇒ the row grows a hover pencil that starts an inline
   * rename. Only instance rows offer it: a rename moves the backing file, so
   * there must be a real pipeline behind the row.
   */
  onRenameStart?: () => void;
  /** Inline-rename state, all owned by the parent (mirror of the run rows). */
  renaming?: boolean;
  renameValue?: string;
  onRenameValueChange?: (value: string) => void;
  onRenameCommit?: () => void;
  onRenameCancel?: () => void;
  /**
   * Fired on the trash affordance. A plain callback on purpose: the parent owns
   * *how* the delete happens (confirm modal + optional #227 cascade for a
   * working pipeline, direct `deleteLibraryPipeline` for a library-only row), so
   * this component never needs to know which mode it is in.
   */
  onDelete: () => void;
  deleteTitle: string;
  testId?: string;
  /**
   * Multi-select (#577). When `onToggleSelect` is provided the row grows a
   * leading select control (a hollow ring on hover, a green check when
   * `checked`) and a `checked` row gets the green left-bar + acc tint — the
   * "selected ≠ open" second channel. Absent ⇒ no select affordance (rows in
   * tests / non-selectable contexts stay byte-for-byte as before).
   */
  checked?: boolean;
  onToggleSelect?: (e: MouseEvent) => void;
  /** ADR-0081 (#31): the `pipeline/item` decorations, at the end of line 2. */
  decorations?: ReactNode;
  /**
   * The validation indicator (CONTEXT.md « Pipeline validé »), bottom-right under
   * the hover actions, always visible.
   */
  indicator?: ReactNode;
  /**
   * #30 — present ⇒ the row grows a « ⋯ » menu after its hover icons: its verbs,
   * the extensions' `pipeline/item/menu` groups (handed this pipeline), then
   * the delete. Only the Pipelines tab's rows offer it.
   */
  menuEntity?: PipelineEntity;
}

/**
 * One row of the Library tab. Both lists in `UnifiedLeftPanel` render through
 * this: the /pipelines rows (openable, star when a Library twin exists, Copy on
 * scope:"library" only) and the library-only rows (passive, always starred,
 * always copyable). They were two near-identical renderers that drifted apart
 * twice — #273 (Copy missing on a scope:"library" row) and #371 (a fresh
 * duplicate stuck in the degraded passive row).
 */
export default function LibraryRow({
  name,
  nodeCount,
  selected = false,
  showDuplicate,
  onOpen,
  onDuplicate,
  onRenameStart,
  renaming = false,
  renameValue = "",
  onRenameValueChange,
  onRenameCommit,
  onRenameCancel,
  onDelete,
  deleteTitle,
  testId,
  checked = false,
  onToggleSelect,
  modified,
  decorations,
  indicator,
  menuEntity,
}: Props) {
  const body = (
    <>
      {onToggleSelect && (
        <SelectControl
          selected={checked}
          label={checked ? `Deselect ${name}` : `Select ${name}`}
          onSelect={onToggleSelect}
        />
      )}
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5">
          {renaming ? (
            <input
              autoFocus
              className="w-full rounded border border-acc bg-bg-3 px-1 py-0.5 font-medium text-fg outline-none"
              style={{ fontSize: "11.5px" }}
              value={renameValue}
              onChange={(e) => onRenameValueChange?.(e.target.value)}
              onBlur={() => onRenameCommit?.()}
              onKeyDown={(e) => {
                if (e.key === "Enter") onRenameCommit?.();
                if (e.key === "Escape") onRenameCancel?.();
              }}
              onClick={(e) => e.stopPropagation()}
              data-testid="library-rename-input"
            />
          ) : (
            <span className="truncate font-medium">{name}</span>
          )}
        </div>
        <div
          className="mt-0.5 flex items-center gap-1.5 text-fg-4"
          style={{ fontSize: "10px" }}
        >
          <span>{nodeCount} nodes</span>
          {modified && (
            <>
              <span>·</span>
              <span>edited {new Date(modified).toLocaleDateString()}</span>
            </>
          )}
          {decorations}
        </div>
      </div>
      {/* Right column, the shape of a run row: line 1 = the hover-revealed
          action icons, line 2 = the validation indicator, always visible. The
          actions line keeps its height at rest, so the indicator never moves
          when the icons appear. */}
      <div className="flex shrink-0 flex-col items-end justify-between gap-1">
      <div className="flex h-4 items-center gap-2" data-testid="library-row-actions">
      {onRenameStart && !renaming && (
        <span
          className="hidden shrink-0 group-hover:inline-flex"
          data-testid="library-rename-button"
          onClick={(e) => {
            e.stopPropagation();
            onRenameStart();
          }}
          role="button"
          title="Rename pipeline"
        >
          <Pencil size={14} className="text-fg-4 transition-colors hover:text-acc" />
        </span>
      )}
      {showDuplicate && (
        <span
          className="hidden shrink-0 group-hover:inline-flex"
          data-testid="library-duplicate-button"
          onClick={(e) => {
            e.stopPropagation();
            onDuplicate?.();
          }}
          role="button"
          title="Duplicate pipeline"
        >
          <Copy size={14} className="text-fg-4 transition-colors hover:text-acc" />
        </span>
      )}
      <span
        className="hidden shrink-0 group-hover:inline-flex"
        onClick={(e) => {
          e.stopPropagation();
          onDelete();
        }}
        role="button"
        title={deleteTitle}
      >
        <Trash2
          size={14}
          className="text-fg-4 transition-colors hover:text-st-failed"
        />
      </span>
      {menuEntity && (
        <PipelineRowMenu
          entity={menuEntity}
          onOpen={onOpen}
          onRenameStart={renaming ? undefined : onRenameStart}
          onDuplicate={showDuplicate ? onDuplicate : undefined}
          onDelete={onDelete}
          deleteTitle={deleteTitle}
        />
      )}
      </div>
      {indicator && <span className="flex shrink-0 items-center">{indicator}</span>}
      </div>
    </>
  );

  if (onOpen) {
    return (
      <button
        onClick={onOpen}
        className={`group flex w-full cursor-pointer items-center gap-2 border-b border-l-2 border-line-soft px-3 py-2 text-left transition-colors ${
          checked
            ? "border-l-acc bg-acc-bg text-fg"
            : selected
              ? "border-l-transparent bg-bg-3 text-fg"
              : "border-l-transparent text-fg-2 hover:bg-bg-3/50"
        }`}
        style={{ fontSize: "11.5px" }}
        data-testid={testId}
      >
        {body}
      </button>
    );
  }

  return (
    <div
      className={`group flex w-full items-center gap-2 border-b border-l-2 border-line-soft px-3 py-2 text-left text-fg-2 transition-colors ${
        checked ? "border-l-acc bg-acc-bg" : "border-l-transparent hover:bg-bg-3/50"
      }`}
      style={{ fontSize: "11.5px" }}
      data-testid={testId}
    >
      {body}
    </div>
  );
}

/** The Pipelines row's « ⋯ » menu (#30). */
function PipelineRowMenu({
  entity,
  onOpen,
  onRenameStart,
  onDuplicate,
  onDelete,
  deleteTitle,
}: {
  entity: PipelineEntity;
  onOpen?: () => void;
  onRenameStart?: () => void;
  onDuplicate?: () => void;
  onDelete: () => void;
  deleteTitle: string;
}) {
  const extra = useMemo(() => ({ entity }), [entity]);
  return (
    <RowMenu label={`More actions for ${entity.name}`} testId="pipeline-row-menu">
      {onOpen && (
        <RowMenuItem data-testid="pipeline-menu-open" onClick={onOpen}>
          <FolderOpen /> Open
        </RowMenuItem>
      )}
      {onRenameStart && (
        <RowMenuItem data-testid="pipeline-menu-rename" onClick={onRenameStart}>
          <Pencil /> Rename
        </RowMenuItem>
      )}
      {onDuplicate && (
        <RowMenuItem data-testid="pipeline-menu-duplicate" onClick={onDuplicate}>
          <Copy /> Duplicate
        </RowMenuItem>
      )}
      <ExtensionMenuGroups location="pipeline/item/menu" extra={extra} itemClassName={ROW_MENU_EXTENSION_ITEM} />
      <DropdownMenuSeparator />
      <RowMenuItem variant="destructive" data-testid="pipeline-menu-delete" onClick={onDelete}>
        <Trash2 /> {deleteTitle}
      </RowMenuItem>
    </RowMenu>
  );
}
