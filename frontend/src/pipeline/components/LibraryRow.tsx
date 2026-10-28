import { useMemo, type KeyboardEvent, type MouseEvent, type ReactNode } from "react";
import { CircleAlert, Copy, FolderOpen, Pencil, Trash2, Workflow } from "lucide-react";
import { ExtensionMenuGroups } from "../../shared/extensions/ExtensionActions";
import type { PipelineEntity } from "../../shared/extensions/contract";
import { DropdownMenuSeparator } from "../../shared/ui/dropdown-menu";
import RowMenu, { ROW_MENU_EXTENSION_ITEM, RowMenuItem } from "../../shared/ui/RowMenu";
import { Tooltip } from "../../shared/ui/kit";
import StatusMark from "../../shared/ui/leftList/StatusMark";
import HoverBar, { RowAction } from "../../shared/ui/leftList/HoverBar";
import { fmtEdited } from "../../shared/ui/leftList/format";
import type { DraftSummary } from "../../shared/generated/DraftSummary";
import { ParticipantStack } from "./ParticipantAvatar";

interface Props {
  name: string;
  nodeCount: number;
  modified?: string | null;
  /** Highlighted as the open editor tab. */
  selected?: boolean;
  showDuplicate: boolean;
  onOpen: () => void;
  onDuplicate?: () => void;
  /**
   * #774 — present ⇒ the row offers an inline rename (its hover bar, its menu,
   * F2). A rename moves the backing file.
   */
  onRenameStart?: () => void;
  /** Inline-rename state, all owned by the parent (mirror of the run rows). */
  renaming?: boolean;
  renameValue?: string;
  onRenameValueChange?: (value: string) => void;
  onRenameCommit?: () => void;
  onRenameCancel?: () => void;
  /**
   * Fired on the trash affordance. The parent owns *how* the delete happens
   * (the confirm modal and its optional #227 cascade).
   */
  onDelete: () => void;
  deleteTitle: string;
  testId?: string;
  /**
   * Multi-select (#577). When `onToggleSelect` is provided the row's lead icon
   * doubles as the select control, and a `checked` row is tinted — the
   * "selected ≠ open" second channel.
   */
  checked?: boolean;
  /** Some pipeline of the list is ticked: every lead shows its box. */
  selecting?: boolean;
  onToggleSelect?: (e: MouseEvent | KeyboardEvent) => void;
  /** Why its file does not parse: the row is marked invalid, the lint in its tooltip. */
  invalid?: string | null;
  /** ADR-0081 (#31): the `pipeline/item` decorations, at the end of line 2. */
  decorations?: ReactNode;
  /** Its validation (CONTEXT.md « Pipeline validé »), at the end of line 1. */
  indicator?: ReactNode;
  /**
   * #30 — present ⇒ the row's « ⋯ » menu: its verbs, the extensions'
   * `pipeline/item/menu` groups (handed this pipeline), then the delete.
   */
  menuEntity?: PipelineEntity;
  /** ADR-0088: its shared draft, who has it open and its changes. */
  draft?: DraftSummary;
  /** ADR-0088: the number of its last version, the file. */
  version?: number;
}

/**
 * One row of the Pipelines tab, two lines: its name (mono), the lint when it
 * is invalid, who has its draft open and its validation; then its node count,
 * version and draft, its marks and its edit date. Rename, Duplicate, Delete
 * and the « ⋯ » menu in the hover bar.
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
  selecting,
  onToggleSelect,
  invalid,
  modified,
  decorations,
  indicator,
  menuEntity,
  draft,
  version,
}: Props) {
  // ADR-0088: a draft with changes counts its own nodes (none counted yet for
  // a draft kept before the count).
  const nodes = draft && draft.changes > 0 && draft.nodes > 0 ? draft.nodes : nodeCount;
  const lead = <Workflow size={14} strokeWidth={1.5} className="lp-dim" aria-hidden />;
  return (
    <div
      role="button"
      tabIndex={0}
      className="lp-row lp-row--flat"
      data-open={selected || undefined}
      data-checked={checked || undefined}
      data-invalid={invalid ? true : undefined}
      aria-current={selected || undefined}
      data-testid={testId}
      onClick={onOpen}
      onKeyDown={(e) => {
        if (e.target !== e.currentTarget) return;
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onOpen();
        } else if (e.key === "F2" && onRenameStart) {
          e.preventDefault();
          onRenameStart();
        }
      }}
    >
      <span className="lp-lead">
        {onToggleSelect ? (
          <StatusMark
            glyph={lead}
            selected={checked}
            selecting={selecting}
            label={checked ? `Deselect ${name}` : `Select ${name}`}
            title={`${checked ? "Deselect" : "Select"} ${name}\nShift+click for a range`}
            onSelect={onToggleSelect}
          />
        ) : (
          lead
        )}
      </span>
      <span className="lp-l1">
        {renaming ? (
          <input
            autoFocus
            className="lp-rename lp-rename--mono"
            aria-label="Name"
            value={renameValue}
            onChange={(e) => onRenameValueChange?.(e.target.value)}
            onBlur={() => onRenameCommit?.()}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Enter") onRenameCommit?.();
              if (e.key === "Escape") onRenameCancel?.();
            }}
            onClick={(e) => e.stopPropagation()}
            data-testid="library-rename-input"
          />
        ) : (
          <span className="lp-name lp-mono" title={name} data-testid="library-row-name">
            {name}
          </span>
        )}
        {/* The lint, only when it is invalid: no mark at all when it is valid. */}
        {invalid && (
          <Tooltip label="Invalid" description={invalid}>
            <span className="lp-lint" role="img" aria-label={`Invalid: ${invalid}`} data-testid="library-row-lint">
              <CircleAlert size={12} strokeWidth={1.75} aria-hidden />
            </span>
          </Tooltip>
        )}
        <span className="lp-end">
          {draft && <ParticipantStack participants={draft.participants} size={14} />}
          {indicator}
        </span>
      </span>
      <span className="lp-l2" data-testid="library-row-meta">
        <span className="lp-num" title={`${nodes} node${nodes === 1 ? "" : "s"}`}>
          {nodes}n
        </span>
        {version != null && (
          <span className="lp-num lp-muted" title={`Version ${version}`} data-testid="library-row-version">
            v{version}
          </span>
        )}
        {/* ADR-0088: before the date. */}
        {draft && draft.changes > 0 && (
          <span className="lp-pipe" data-testid="library-row-draft">
            draft: {draft.changes === 1 ? "1 change" : `${draft.changes} changes`}
          </span>
        )}
        <span className="lp-end">
          {decorations && <span className="lp-deco">{decorations}</span>}
          {modified && (
            <span
              className="lp-num lp-muted"
              title={`Edited ${new Date(modified).toLocaleString()}`}
              data-testid="library-row-edited"
            >
              {fmtEdited(modified)}
            </span>
          )}
        </span>
      </span>
      {!renaming && (
        <HoverBar testId="library-row-actions">
          {onRenameStart && (
            <RowAction icon={Pencil} label="Rename pipeline" testId="library-rename-button" onClick={onRenameStart} />
          )}
          {showDuplicate && onDuplicate && (
            <RowAction icon={Copy} label="Duplicate pipeline" testId="library-duplicate-button" onClick={onDuplicate} />
          )}
          <RowAction icon={Trash2} label={deleteTitle} danger testId="library-delete-button" onClick={onDelete} />
          {menuEntity && (
            <PipelineRowMenu
              entity={menuEntity}
              onOpen={onOpen}
              onRenameStart={onRenameStart}
              onDuplicate={showDuplicate ? onDuplicate : undefined}
              onDelete={onDelete}
              deleteTitle={deleteTitle}
            />
          )}
        </HoverBar>
      )}
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
  onOpen: () => void;
  onRenameStart?: () => void;
  onDuplicate?: () => void;
  onDelete: () => void;
  deleteTitle: string;
}) {
  const extra = useMemo(() => ({ entity }), [entity]);
  return (
    <RowMenu label={`More actions for ${entity.name}`} testId="pipeline-row-menu">
      <RowMenuItem data-testid="pipeline-menu-open" onClick={onOpen}>
        <FolderOpen /> Open
      </RowMenuItem>
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
