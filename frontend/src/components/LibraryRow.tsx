import type { MouseEvent } from "react";
import { Copy, Trash2 } from "lucide-react";
import type { PipelineScope } from "../types";
import SelectControl from "./SelectControl";

interface Props {
  name: string;
  scope: PipelineScope;
  nodeCount: number;
  modified?: string | null;
  /** Star badge — the visible "this name is in your Library" link (#227). */
  starred: boolean;
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
  scope,
  nodeCount,
  starred,
  selected = false,
  showDuplicate,
  onOpen,
  onDuplicate,
  onDelete,
  deleteTitle,
  testId,
  checked = false,
  onToggleSelect,
  modified,
}: Props) {
  void scope;
  void starred;

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
          <span className="truncate font-medium">{name}</span>
        </div>
        <div
          className="mt-0.5 flex items-center gap-1.5 text-fg-4"
          style={{ fontSize: "10px" }}
        >
          <span>{nodeCount} nodes</span>
          {modified && (
            <>
              <span>·</span>
              <span>{new Date(modified).toLocaleDateString()}</span>
            </>
          )}
        </div>
      </div>
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
