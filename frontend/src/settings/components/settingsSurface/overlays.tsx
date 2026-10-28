import { useEffect, useRef } from "react";
import { SETTINGS_CATEGORIES, type DirtyRollup } from "../../model/settingsCategories";

export function ConfirmCloseDialog({
  rollup,
  saving,
  onKeep,
  onDiscard,
  onSaveAndClose,
}: {
  rollup: DirtyRollup;
  saving: boolean;
  onKeep: () => void;
  onDiscard: () => void;
  onSaveAndClose: () => void;
}) {
  const keepRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    keepRef.current?.focus();
  }, []);

  const places = SETTINGS_CATEGORIES.flatMap((category) =>
    category.sections
      .filter((section) => rollup.sections.has(section.id))
      .map((section) => `${category.label} › ${section.label}`),
  );
  const count = rollup.fields.size;

  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-scrim"
      onClick={onKeep}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-confirm-title"
        className="w-[380px] rounded-lg border border-line bg-surface-2 p-4 shadow-xl"
        onClick={(e) => e.stopPropagation()}
        data-testid="settings-confirm-close"
      >
        <h3 id="settings-confirm-title" className="font-semibold text-text" style={{ fontSize: "13px" }}>
          Discard unsaved changes?
        </h3>
        <p className="mt-2 text-text-3" style={{ fontSize: "11px" }}>
          You edited{" "}
          {places.map((place, index) => (
            <span key={place}>
              {index > 0 && ", "}
              <strong className="text-text-2">{place}</strong>
            </span>
          ))}{" "}
          ({count} field{count === 1 ? "" : "s"}). Closing Settings without saving drops the
          edit{count === 1 ? "" : "s"}.
        </p>
        <div className="mt-4 flex items-center justify-end gap-2">
          <button
            ref={keepRef}
            type="button"
            onClick={onKeep}
            data-testid="settings-confirm-keep"
            className="rounded-md border border-line-strong bg-surface-2 px-3 py-1.5 text-text-2 hover:bg-active"
            style={{ fontSize: "11.5px" }}
          >
            Keep editing
          </button>
          <button
            type="button"
            onClick={onDiscard}
            data-testid="settings-confirm-discard"
            className="rounded-md border border-st-failed/40 bg-st-failed/14 px-3 py-1.5 text-st-failed hover:border-st-failed"
            style={{ fontSize: "11.5px" }}
          >
            Discard
          </button>
          <button
            type="button"
            onClick={onSaveAndClose}
            disabled={saving}
            data-testid="settings-confirm-save-close"
            className="rounded-md bg-primary px-3 py-1.5 font-medium text-primary-ink hover:bg-primary-hi disabled:opacity-40"
            style={{ fontSize: "11.5px" }}
          >
            {saving ? "Saving…" : "Save & close"}
          </button>
        </div>
      </div>
    </div>
  );
}
