import type { RefObject } from "react";
import { SETTINGS_CATEGORIES } from "../../model/settingsCategories";
import type { SettingsForm } from "../../store/useSettingsForm";

/**
 * The Save footer, visible on every core category (not on an extension's page, which owns
 * its writes): where the unsaved changes are, the daemon's refusal next to Save, Cancel.
 */
export default function SettingsFooter({
  form,
  saveButtonRef,
  loaded,
  onCancel,
}: {
  form: Omit<SettingsForm, "saveButtonRef">;
  /** Focused when a save fails, so the refusal is read next to the button. */
  saveButtonRef: RefObject<HTMLButtonElement | null>;
  /** `GET /settings` answered: Save has something to compare against. */
  loaded: boolean;
  onCancel: () => void;
}) {
  const { saveState, isDirty, rollup, missingDefaultProfile } = form;
  const dirtyCategory = SETTINGS_CATEGORIES.find((item) => rollup.categories.has(item.id));
  const dirtyCount = rollup.fields.size;
  const footerText =
    saveState.status === "saved"
      ? "Saved"
      : isDirty && dirtyCategory
        ? `Unsaved changes in ${dirtyCategory.label} (${dirtyCount} field${dirtyCount === 1 ? "" : "s"})`
        : "No unsaved changes";

  return (
    <div
      className="flex items-center gap-3 border-t border-line px-4 py-3"
      data-testid="settings-footer"
    >
      <span
        className={isDirty || saveState.status === "saved" ? "text-st-await" : "text-fg-4"}
        style={{ fontSize: "11px" }}
        data-testid="settings-footer-status"
        data-dirty={isDirty ? "true" : undefined}
      >
        {footerText}
      </span>
      <div className="ml-auto flex items-center gap-2">
        {saveState.status === "error" && (
          <span
            role="alert"
            className="text-st-failed"
            style={{ fontSize: "11px" }}
            data-testid="settings-error"
          >
            Save failed: {saveState.message}
          </span>
        )}
        <button
          type="button"
          onClick={onCancel}
          data-testid="settings-cancel"
          className="rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
          style={{ fontSize: "11.5px" }}
        >
          Cancel
        </button>
        <button
          ref={saveButtonRef}
          type="button"
          onClick={() => void form.handleSave()}
          // Disabled while the form is clean (#691): on a clean page the only enabled primary
          // is a panel's own. A dangling stored default keeps it clickable so the blocker
          // is read next to the button, not guessed.
          disabled={saveState.status === "saving" || !loaded || (!isDirty && !missingDefaultProfile)}
          className="rounded-md bg-acc px-3 py-1.5 font-medium text-on-acc transition-colors hover:bg-acc-dim disabled:opacity-40"
          style={{ fontSize: "11.5px" }}
          data-testid="settings-save"
        >
          {saveState.status === "saving" ? "Saving…" : "Save"}
        </button>
      </div>
    </div>
  );
}
