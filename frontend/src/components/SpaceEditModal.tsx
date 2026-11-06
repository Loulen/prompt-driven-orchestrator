import { useState } from "react";
import type { Space } from "../types";
import { ApiError, createSpace, deleteSpace, updateSpace } from "../api";
import {
  firstFreeSpaceColor,
  isSpaceColor,
  SPACE_COLORS,
  SPACE_COLOR_LABELS,
  spaceColorVar,
  type SpaceColor,
} from "../lib/spacePalette";

/**
 * Create, rename, recolour or delete an **Espace** (#1041, CONTEXT.md « Espace »)
 * — opened by « + Space » in the Runs header (create) or by the pencil of an
 * Espace header (edit). Prior art: `ProjectEditModal`. An Espace is shared by the
 * whole instance and carries no configuration: a name and a colour, nothing else.
 *
 * Deleting sends the Espace's Runs back to « Sans espace » ("No space"); nothing
 * else is lost, so a single confirm step is enough.
 */
export default function SpaceEditModal({
  space,
  spaces,
  onClose,
  onSaved,
}: {
  /** The Espace being edited, or `null` to create one. */
  space: Space | null;
  /** Every Espace, to pre-select the first free colour on create. */
  spaces: Space[];
  onClose: () => void;
  onSaved: () => void;
}) {
  const [name, setName] = useState(space?.name ?? "");
  const [color, setColor] = useState<SpaceColor>(() =>
    space && isSpaceColor(space.color)
      ? space.color
      : firstFreeSpaceColor(spaces.map((s) => s.color)),
  );
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const trimmed = name.trim();
  const canSave = trimmed.length > 0 && !submitting;

  async function run(action: () => Promise<unknown>, fallback: string) {
    setSubmitting(true);
    setError(null);
    try {
      await action();
      onSaved();
      onClose();
    } catch (e) {
      setError(e instanceof ApiError || e instanceof Error ? e.message : fallback);
      setSubmitting(false);
    }
  }

  function handleSave() {
    if (!canSave) return;
    void run(
      () =>
        space
          ? updateSpace(space.id, { name: trimmed, color })
          : createSpace(trimmed, color),
      "failed to save space",
    );
  }

  function handleDelete() {
    if (!space || submitting) return;
    void run(() => deleteSpace(space.id), "failed to delete space");
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      data-testid="space-edit-backdrop"
      onClick={onClose}
    >
      <div
        className="flex w-[380px] max-w-[90vw] flex-col rounded-lg border border-line bg-bg-4"
        style={{ fontSize: "12px" }}
        role="dialog"
        aria-label={space ? "Edit space" : "New space"}
        data-testid="space-edit-modal"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="border-b border-line px-4 pb-3 pt-4">
          <div className="mb-1 font-medium text-fg">{space ? "Edit space" : "New space"}</div>
          <p className="text-fg-4" style={{ fontSize: "11px" }}>
            Sort this instance&apos;s runs under a name, one space per person or per
            topic. Everyone on the instance sees the same spaces.
          </p>
        </div>

        <div className="px-4 pt-3">
          <label className="mb-1 block text-fg-3" style={{ fontSize: "11px" }} htmlFor="space-name-input">
            Name
          </label>
          <input
            id="space-name-input"
            autoFocus
            value={name}
            onChange={(e) => {
              setName(e.target.value);
              setError(null);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") handleSave();
              if (e.key === "Escape") onClose();
            }}
            data-testid="space-name-input"
            className="mb-3 w-full rounded border border-line-strong bg-bg-3 px-2 py-1.5 text-fg outline-none focus:border-acc"
            style={{ fontSize: "11px" }}
          />

          <div className="mb-1 text-fg-3" style={{ fontSize: "11px" }} id="space-color-label">
            Colour
          </div>
          <div
            role="radiogroup"
            aria-labelledby="space-color-label"
            className="mb-3 flex flex-wrap gap-2"
            data-testid="space-color-picker"
          >
            {SPACE_COLORS.map((c) => {
              const selected = c === color;
              return (
                <button
                  key={c}
                  type="button"
                  role="radio"
                  aria-checked={selected}
                  aria-label={SPACE_COLOR_LABELS[c]}
                  title={SPACE_COLOR_LABELS[c]}
                  data-testid={`space-color-${c}`}
                  onClick={() => setColor(c)}
                  className={`h-6 w-6 cursor-pointer rounded border-2 transition-colors ${
                    selected ? "border-fg" : "border-transparent hover:border-line-strong"
                  }`}
                  style={{ background: spaceColorVar(c) }}
                />
              );
            })}
          </div>
        </div>

        <div className="border-t border-line px-4 py-3">
          {error && (
            <div
              className="mb-3 rounded border border-st-failed/40 bg-st-failed/10 px-2 py-1.5 text-st-failed"
              style={{ fontSize: "11px" }}
              data-testid="space-edit-error"
            >
              {error}
            </div>
          )}

          {confirmDelete && space ? (
            <div data-testid="space-delete-confirm">
              <p className="mb-2 text-fg-2" style={{ fontSize: "11px" }}>
                Delete space “{space.name}”? Its runs move to No space; nothing else is
                lost.
              </p>
              <div className="flex justify-end gap-2">
                <button
                  onClick={() => setConfirmDelete(false)}
                  className="cursor-pointer rounded border border-line-strong px-3 py-1 text-fg-3 transition-colors hover:bg-bg-3"
                  style={{ fontSize: "11px" }}
                  data-testid="space-delete-cancel"
                >
                  Keep it
                </button>
                <button
                  onClick={handleDelete}
                  disabled={submitting}
                  className="cursor-pointer rounded bg-st-failed px-3 py-1 font-medium text-white transition-colors hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
                  style={{ fontSize: "11px" }}
                  data-testid="space-delete-confirm-button"
                >
                  {submitting ? "Deleting…" : "Delete space"}
                </button>
              </div>
            </div>
          ) : (
            <div className="flex items-center gap-2">
              {space && (
                <button
                  onClick={() => setConfirmDelete(true)}
                  className="cursor-pointer rounded px-2 py-1 text-st-failed transition-colors hover:bg-bg-3"
                  style={{ fontSize: "11px" }}
                  data-testid="space-delete"
                >
                  Delete
                </button>
              )}
              <div className="ml-auto flex gap-2">
                <button
                  onClick={onClose}
                  className="cursor-pointer rounded border border-line-strong px-3 py-1 text-fg-3 transition-colors hover:bg-bg-3"
                  style={{ fontSize: "11px" }}
                  data-testid="space-edit-cancel"
                >
                  Cancel
                </button>
                <button
                  onClick={handleSave}
                  disabled={!canSave}
                  className="cursor-pointer rounded bg-acc px-3 py-1 font-medium text-on-acc transition-colors hover:bg-acc-dim disabled:cursor-not-allowed disabled:opacity-50"
                  style={{ fontSize: "11px" }}
                  data-testid="space-edit-save"
                >
                  {submitting ? "Saving…" : space ? "Save" : "Create"}
                </button>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
