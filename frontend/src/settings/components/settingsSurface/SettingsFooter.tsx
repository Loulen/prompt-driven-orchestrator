import type { RefObject } from "react";
import { CircleX } from "lucide-react";
import { Button, Kbd } from "../../../shared/ui/kit";
import type { SettingsForm } from "../../store/useSettingsForm";

/**
 * The Save bar under the core categories (not on an extension's page, which owns its
 * writes): shown while the instance form has something to say — how many settings are
 * unsaved, the refusal next to Save, « Saved » once written. Discard drops the draft.
 */
export default function SettingsFooter({
  form,
  saveButtonRef,
  loaded,
  onDiscard,
}: {
  form: Omit<SettingsForm, "saveButtonRef">;
  /** Focused when a save fails, so the refusal is read next to the button. */
  saveButtonRef: RefObject<HTMLButtonElement | null>;
  /** `GET /settings` answered: Save has something to compare against. */
  loaded: boolean;
  /** Drops the draft. */
  onDiscard: () => void;
}) {
  const { saveState, isDirty, rollup, missingDefaultProfile } = form;
  // A dangling stored default keeps Save reachable so the blocker is read next to it.
  if (!isDirty && !missingDefaultProfile && saveState.status === "idle") return null;
  const dirtyCount = rollup.fields.size;

  return (
    <footer className="d-bar set-savebar" data-testid="settings-footer">
      {isDirty && <span className="set-dirty" aria-hidden />}
      <span
        className={saveState.status === "saved" ? "set-ok" : undefined}
        data-testid="settings-footer-status"
        data-dirty={isDirty ? "true" : undefined}
      >
        {saveState.status === "saved" && !isDirty ? "Saved" : isDirty ? `${dirtyCount} unsaved` : ""}
      </span>
      {saveState.status === "error" && (
        <span role="alert" className="d-err set-ellipsis" title={saveState.message} data-testid="settings-error">
          <CircleX size={12} strokeWidth={1.5} aria-hidden />
          {saveState.message}
        </span>
      )}
      <span className="d-grow" />
      {isDirty && (
        <Button onClick={onDiscard} data-testid="settings-discard">
          Discard
        </Button>
      )}
      <Button
        ref={saveButtonRef}
        tone="primary"
        onClick={() => void form.handleSave()}
        disabled={saveState.status === "saving" || !loaded || (!isDirty && !missingDefaultProfile)}
        title="Save (Ctrl+S)"
        data-testid="settings-save"
      >
        {saveState.status === "saving" ? "Saving…" : "Save"}
        <Kbd>Ctrl S</Kbd>
      </Button>
    </footer>
  );
}
