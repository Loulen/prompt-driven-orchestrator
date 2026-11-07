import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { announceSkillTiersChanged } from "../../skill/store/useSkillTiers";
import type { UpdateSettingsRequest } from "../../shared/generated/UpdateSettingsRequest";
import {
  buildPatch,
  computeDirty,
  missingDefaultProfile as isDefaultProfileMissing,
  seedFrom,
  type DraftValues,
} from "../model/draft";
import type { InstanceSettings } from "../model/settings";
import { rollupDirty, type DirtyRollup } from "../model/settingsCategories";
import { announceSettingsChanged } from "./useSettings";
import { useRefusal } from "../../shared/authorization/useRefusals";

/** The Operation behind Save (`PUT /settings`). */
const UPDATE_SETTINGS = { resource: "instance_settings", verb: "update" } as const;

export type SaveState =
  | { status: "idle" }
  | { status: "saving" }
  | { status: "saved" }
  | { status: "error"; message: string };

/**
 * The instance form of the Settings surface: the draft, kept as **overrides** on top of the
 * loaded settings (`{}` = clean) so a category switch never touches it, the dirty set derived
 * from the same comparisons the PUT payload uses, and the one explicit Save.
 */
export function useSettingsForm({
  settings,
  save,
  onSaved,
}: {
  settings: InstanceSettings | null;
  save: (patch: UpdateSettingsRequest) => Promise<InstanceSettings>;
  onSaved?: () => void;
}) {
  const [draft, setDraft] = useState<Partial<DraftValues>>({});
  const [saveState, setSaveState] = useState<SaveState>({ status: "idle" });
  const saveButtonRef = useRef<HTMLButtonElement>(null);
  const savedTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(
    () => () => {
      if (savedTimer.current) clearTimeout(savedTimer.current);
    },
    [],
  );

  const seeded = useMemo(() => (settings ? seedFrom(settings) : null), [settings]);
  const values: DraftValues | null = useMemo(
    () => (seeded ? { ...seeded, ...draft } : null),
    [seeded, draft],
  );
  const rollup: DirtyRollup = useMemo(
    () => rollupDirty(values && settings ? computeDirty(values, settings) : new Set()),
    [values, settings],
  );
  const isDirty = rollup.fields.size > 0;

  const setField = useCallback(<K extends keyof DraftValues>(key: K, value: DraftValues[K]) => {
    setDraft((current) => ({ ...current, [key]: value }));
    setSaveState((current) => (current.status === "saved" ? { status: "idle" } : current));
  }, []);

  const missingDefaultProfile = !!values && !!settings && isDefaultProfileMissing(values, settings);
  // CORE-205: why the caller may not save the instance settings (Save greyed, saying it), else absent.
  const saveRefused = useRefusal(UPDATE_SETTINGS, settings !== null);

  /** PUT the changed fields. Resolves `true` when the draft is clean afterwards. */
  const handleSave = async (): Promise<boolean> => {
    if (saveState.status === "saving") return false;
    // A refused Save sends nothing: the button says why (Ctrl+S included).
    if (saveRefused) {
      saveButtonRef.current?.focus();
      return false;
    }
    const built = values && settings ? buildPatch(values, settings, rollup.fields) : { patch: {} };
    if ("error" in built) {
      setSaveState({ status: "error", message: built.error });
      saveButtonRef.current?.focus();
      return false;
    }
    if (Object.keys(built.patch).length === 0) {
      setSaveState({ status: "idle" });
      return true;
    }
    setSaveState({ status: "saving" });
    try {
      await save(built.patch);
      if (built.patch.skills) announceSkillTiersChanged();
      announceSettingsChanged();
      onSaved?.();
      setDraft({});
      setSaveState({ status: "saved" });
      if (savedTimer.current) clearTimeout(savedTimer.current);
      savedTimer.current = setTimeout(() => {
        setSaveState((current) => (current.status === "saved" ? { status: "idle" } : current));
      }, 2000);
      return true;
    } catch (e) {
      setSaveState({ status: "error", message: e instanceof Error ? e.message : String(e) });
      saveButtonRef.current?.focus();
      return false;
    }
  };

  /** Drop the draft and any save outcome (a close). */
  const reset = () => {
    setDraft({});
    setSaveState({ status: "idle" });
  };

  return {
    values,
    rollup,
    isDirty,
    setField,
    missingDefaultProfile,
    saveState,
    saveRefused,
    handleSave,
    reset,
    saveButtonRef,
  };
}

export type SettingsForm = ReturnType<typeof useSettingsForm>;
