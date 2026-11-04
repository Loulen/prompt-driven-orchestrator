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
import { useAppearancePreview, useElementMarks } from "../../hooks/useElementMarks";
import { useAppearance } from "../../hooks/useAppearance";
import { isVisible } from "../../lib/appearance";
import { useHideableContributions } from "../../shared/extensions/hideableContributions";
import { hasSecondLine, isSecondLineElement, PIPELINE_ROW, PIPELINE_ROW_LOCATIONS } from "../model/pipelineRowElements";

interface Props {
  name: string;
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
 * is invalid, who has its draft open and its validation; then its version and
 * draft, its marks and its edit date. Rename, Duplicate, Delete
 * and the « ⋯ » menu in the hover bar.
 *
 * Every element but its name, its select control and its lint is an Élément
 * masquable (`model/pipelineRowElements.ts`; who has its draft open goes with
 * the draft marker): one the Apparence hides is not rendered, and the row takes
 * one line when its whole second line is hidden — the Apparence decides, not
 * the Pipeline, so every row of the list keeps one height. In a preview of
 * Settings › Appearance, pointing to an element of a hidden second line brings
 * that line back as a ghost (`data-ghost-line`); a preview's row is inert: no
 * hover bar, out of the tab order.
 */
export default function LibraryRow({
  name,
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
  const lead = <Workflow size={14} strokeWidth={1.5} className="lp-dim" aria-hidden />;
  const preview = useAppearancePreview();
  const marksOf = useElementMarks();
  const contributions = useHideableContributions("pipelines.list", PIPELINE_ROW_LOCATIONS);
  // From what the Apparence shows alone: a preview's ghost never keeps a line.
  const appearance = useAppearance();
  const contributionIds = contributions.map((c) => c.id);
  const secondLine = hasSecondLine((id) => isVisible(appearance, id), contributionIds);
  const pointed = preview?.pointed ?? null;
  // A preview points to an element of a hidden second line this row can draw.
  const ghostLine =
    !secondLine &&
    pointed !== null &&
    isSecondLineElement(pointed, contributionIds) &&
    (pointed !== PIPELINE_ROW.version || version != null) &&
    (pointed !== PIPELINE_ROW.draft || (draft?.changes ?? 0) > 0) &&
    (pointed !== PIPELINE_ROW.modified || Boolean(modified)) &&
    !(preview?.sample && contributionIds.includes(pointed));
  const versionMarks = marksOf(PIPELINE_ROW.version);
  const draftMarks = marksOf(PIPELINE_ROW.draft);
  const modifiedMarks = marksOf(PIPELINE_ROW.modified);
  const validationMarks = marksOf(PIPELINE_ROW.validation);
  return (
    <div
      role="button"
      tabIndex={preview ? -1 : 0}
      className="lp-row lp-row--flat"
      data-lines={secondLine ? 2 : 1}
      data-ghost-line={ghostLine || undefined}
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
          {draft && draft.participants.length > 0 && draftMarks && (
            <span className="inline-flex" data-testid="library-row-participants" {...draftMarks}>
              <ParticipantStack participants={draft.participants} size="sm" />
            </span>
          )}
          {indicator && validationMarks && (
            <span className="inline-flex" data-testid="library-row-validation" {...validationMarks}>
              {indicator}
            </span>
          )}
        </span>
      </span>
      {(secondLine || ghostLine) && (
        <span className="lp-l2" data-testid="library-row-meta">
          {version != null && versionMarks && (
            <span
              className="lp-num lp-muted"
              title={`Version ${version}`}
              data-testid="library-row-version"
              {...versionMarks}
            >
              v{version}
            </span>
          )}
          {/* ADR-0088: before the date. */}
          {draft && draft.changes > 0 && draftMarks && (
            <span className="lp-pipe" data-testid="library-row-draft" {...draftMarks}>
              draft: {draft.changes === 1 ? "1 change" : `${draft.changes} changes`}
            </span>
          )}
          <span className="lp-end">
            {decorations && <span className="lp-deco">{decorations}</span>}
            {modified && modifiedMarks && (
              <span
                className="lp-num lp-muted"
                title={`Edited ${new Date(modified).toLocaleString()}`}
                data-testid="library-row-edited"
                {...modifiedMarks}
              >
                {fmtEdited(modified)}
              </span>
            )}
          </span>
        </span>
      )}
      {!renaming && !preview && (
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
