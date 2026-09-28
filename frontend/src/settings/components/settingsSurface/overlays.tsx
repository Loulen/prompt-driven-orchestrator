import { useEffect, useRef, type ReactNode } from "react";
import { X } from "lucide-react";
import { SETTINGS_CATEGORIES, type DirtyRollup } from "../../model/settingsCategories";

/**
 * The shell's right drawer hosting the skill bank (#690 decision 5, kept by #691 for this
 * one panel): the rail stays visible, Escape returns to Settings › Skills first, and the
 * Save footer hides under it so its own writes and Save are never both in view.
 */
export function SettingsDrawer({ onClose, children }: { onClose: () => void; children: ReactNode }) {
  const kind = "skills";
  return (
    <aside
      className="absolute bottom-0 right-0 top-14 z-20 flex w-[min(880px,90vw)] flex-col border-l border-line bg-bg-4 shadow-2xl"
      data-testid="settings-drawer"
      data-drawer={kind}
    >
      <div className="flex items-center gap-3 border-b border-line px-4 py-3">
        <h3 className="font-semibold text-fg" style={{ fontSize: "13px" }}>
          Skill bank
        </h3>
        <span className="ml-auto text-fg-4" style={{ fontSize: "10.5px" }}>
          saves as you go · Esc returns to Settings
        </span>
        <button
          type="button"
          onClick={onClose}
          aria-label="Close panel"
          data-testid="settings-drawer-close"
          className="grid h-6 w-6 shrink-0 place-items-center rounded text-fg-3 transition-colors hover:bg-bg-5 hover:text-fg"
        >
          <X size={14} />
        </button>
      </div>
      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">{children}</div>
    </aside>
  );
}

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
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/50"
      onClick={onKeep}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-confirm-title"
        className="w-[380px] rounded-lg border border-line bg-bg-4 p-4 shadow-xl"
        onClick={(e) => e.stopPropagation()}
        data-testid="settings-confirm-close"
      >
        <h3 id="settings-confirm-title" className="font-semibold text-fg" style={{ fontSize: "13px" }}>
          Discard unsaved changes?
        </h3>
        <p className="mt-2 text-fg-3" style={{ fontSize: "11px" }}>
          You edited{" "}
          {places.map((place, index) => (
            <span key={place}>
              {index > 0 && ", "}
              <strong className="text-fg-2">{place}</strong>
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
            className="rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 hover:bg-bg-5"
            style={{ fontSize: "11.5px" }}
          >
            Keep editing
          </button>
          <button
            type="button"
            onClick={onDiscard}
            data-testid="settings-confirm-discard"
            className="rounded-md border border-st-failed/40 bg-st-failed-bg px-3 py-1.5 text-st-failed hover:border-st-failed"
            style={{ fontSize: "11.5px" }}
          >
            Discard
          </button>
          <button
            type="button"
            onClick={onSaveAndClose}
            disabled={saving}
            data-testid="settings-confirm-save-close"
            className="rounded-md bg-acc px-3 py-1.5 font-medium text-on-acc hover:bg-acc-dim disabled:opacity-40"
            style={{ fontSize: "11.5px" }}
          >
            {saving ? "Saving…" : "Save & close"}
          </button>
        </div>
      </div>
    </div>
  );
}
