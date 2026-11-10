import { Check, ChevronDown, ChevronRight, Pencil } from "lucide-react";
import { Tooltip } from "../kit";

interface Props {
  /** The group's name, drawn in capitals. */
  label: string;
  /** Its full name and repositories, on hover. */
  title?: string;
  /** Rows in the group: the count stays visible, folded or not. */
  count: number;
  /** A Projet deleted since: its name in italics. */
  deleted?: boolean;
  /** Folded: absent ⇒ the group does not fold. */
  collapsed?: boolean;
  onToggle?: () => void;
  /** Rows awaiting a person, marked while folded. */
  awaiting?: number;
  /** Every row of the group is ticked. */
  allSelected: boolean;
  onSelectAll: () => void;
  /** The pencil: Settings › Projects on this group; absent ⇒ none. */
  onEdit?: () => void;
  editLabel?: string;
  /** `data-testid` prefix: `<prefix>-chevron`, `-select-all`, `-label`, `-count`, `-pencil`, `-awaiting`. */
  testPrefix: string;
  /** `data-testid` of the label, when it predates the prefix. */
  labelTestId?: string;
}

/**
 * The header of a Projet group in a list of the left panel: caret, name in
 * capitals, the count in a chip; the pencil and « select all » on hover. The
 * whole bar folds the group.
 */
export default function GroupHeader({
  label,
  title,
  count,
  deleted,
  collapsed,
  onToggle,
  awaiting = 0,
  allSelected,
  onSelectAll,
  onEdit,
  editLabel = "Edit project",
  testPrefix,
  labelTestId,
}: Props) {
  const folds = onToggle !== undefined;
  const toggle = () => onToggle?.();
  return (
    <div
      className="lp-group"
      role={folds ? "button" : undefined}
      tabIndex={folds ? 0 : undefined}
      aria-expanded={folds ? !collapsed : undefined}
      aria-label={folds ? `${collapsed ? "Expand" : "Collapse"} ${label}` : undefined}
      title={title}
      data-testid={folds ? `${testPrefix}-chevron` : undefined}
      onClick={folds ? toggle : undefined}
      onKeyDown={(e) => {
        if (!folds || e.target !== e.currentTarget) return;
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          toggle();
        }
      }}
    >
      {folds &&
        (collapsed ? (
          <ChevronRight size={14} strokeWidth={1.5} className="lp-group__caret" aria-hidden />
        ) : (
          <ChevronDown size={14} strokeWidth={1.5} className="lp-group__caret" aria-hidden />
        ))}
      <span
        className="d-caps lp-group__name"
        data-deleted={deleted || undefined}
        data-testid={labelTestId ?? `${testPrefix}-label`}
      >
        {label}
      </span>
      {collapsed && awaiting > 0 && (
        <Tooltip label={`${awaiting} awaiting you`}>
          <span className="lp-group__await" data-testid={`${testPrefix}-awaiting`} aria-label={`${awaiting} awaiting you`} />
        </Tooltip>
      )}
      <span className="lp-group__push" />
      {onEdit && (
        <Tooltip label={editLabel}>
          <button
            type="button"
            className="d-icon d-icon--sm lp-hover-in"
            aria-label={editLabel}
            data-testid={`${testPrefix}-pencil`}
            onClick={(e) => {
              e.stopPropagation();
              onEdit();
            }}
          >
            <Pencil size={12} strokeWidth={1.5} aria-hidden />
          </button>
        </Tooltip>
      )}
      <span
        role="checkbox"
        aria-checked={allSelected}
        aria-label={`Select all in ${label}`}
        title="Select all"
        tabIndex={0}
        className="lp-box lp-hover-in"
        data-testid={`${testPrefix}-select-all`}
        onClick={(e) => {
          e.stopPropagation();
          onSelectAll();
        }}
        onKeyDown={(e) => {
          if (e.key !== " " && e.key !== "Enter") return;
          e.preventDefault();
          e.stopPropagation();
          onSelectAll();
        }}
      >
        {allSelected && <Check size={11} strokeWidth={3} aria-hidden />}
      </span>
      <span className="d-chip lp-group__count" data-testid={`${testPrefix}-count`}>
        {count}
      </span>
    </div>
  );
}
