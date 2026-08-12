import { Copy, Star, Trash2 } from "lucide-react";
import type { PipelineScope } from "../types";

const SCOPE_BADGE: Record<PipelineScope, { label: string; cls: string }> = {
  repo: { label: "repo", cls: "border-acc text-acc" },
  user: { label: "user", cls: "border-st-await text-st-await" },
  library: { label: "library", cls: "border-st-await text-st-await" },
};

interface Props {
  name: string;
  scope: PipelineScope;
  nodeCount: number;
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
}: Props) {
  const badge = SCOPE_BADGE[scope];

  const body = (
    <>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5">
          {starred && (
            <Star
              size={10}
              className="shrink-0 fill-acc text-acc"
              data-testid="left-panel-star"
            />
          )}
          <span className="truncate font-medium">{name}</span>
        </div>
        <div
          className="mt-0.5 flex items-center gap-1.5 text-fg-4"
          style={{ fontSize: "10px" }}
        >
          <span>{nodeCount} nodes</span>
        </div>
      </div>
      <span
        className={`shrink-0 rounded border px-1 py-px group-hover:hidden ${badge.cls}`}
        style={{ fontSize: "9px", fontWeight: 500 }}
      >
        {badge.label}
      </span>
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
        className={`group flex w-full cursor-pointer items-center gap-2 border-b border-line-soft px-3 py-2 text-left transition-colors ${
          selected ? "bg-bg-3 text-fg" : "text-fg-2 hover:bg-bg-3/50"
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
      className="group flex w-full items-center gap-2 border-b border-line-soft px-3 py-2 text-left text-fg-2 transition-colors hover:bg-bg-3/50"
      style={{ fontSize: "11.5px" }}
      data-testid={testId}
    >
      {body}
    </div>
  );
}
