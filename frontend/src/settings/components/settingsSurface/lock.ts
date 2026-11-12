/**
 * The instance form, read only (CORE-248): without `instance_settings` × `update`, every
 * field of the instance categories is greyed, the refusal's sentence in its tooltip. No
 * banner: the grey already says it. A section that is not the instance form (its own
 * persistence, this device, your account, read only) and a row wearing its own scope keep
 * their controls.
 */

import { createContext, useContext } from "react";
import type { OperationQuestion } from "../../../shared/authorization/api";
import { useRefusal } from "../../../shared/authorization/useRefusals";
import type { SettingsSection } from "../../model/settingsCategories";
import type { Scope } from "./fields";

/** The sentence of the refusal of `instance_settings` × `update`; `undefined` when allowed (or not answered yet). */
export const InstanceLockContext = createContext<string | undefined>(undefined);

/** The lock of the section a row is in: the instance's, or none. */
export const SectionLockContext = createContext<string | undefined>(undefined);

/** Whether `section` holds fields of the instance form. */
export function holdsInstanceFields(
  section: Pick<SettingsSection, "readOnly" | "ownPersistence" | "deviceLocal" | "account">,
): boolean {
  return !section.readOnly && !section.ownPersistence && !section.deviceLocal && !section.account;
}

/** Why a row's control is greyed, `undefined` when it is not. */
export function useRowLock(scope?: Scope): string | undefined {
  const lock = useContext(SectionLockContext);
  return scope ? undefined : lock;
}

/** Changing the instance form. */
const UPDATE_INSTANCE: OperationQuestion = { resource: "instance_settings", verb: "update" };

/**
 * Why the caller may not change the instance form, `undefined` when it may (or before the
 * answer): its fields are then greyed, nothing becomes dirty, so the Save bar never shows.
 */
export function useInstanceLock(open: boolean): string | undefined {
  return useRefusal(UPDATE_INSTANCE, open);
}

/** `setField`, or while the form is locked a setter whose change goes nowhere. */
export function lockedSetter<F extends (...args: never[]) => void>(setField: F, lock: string | undefined): F {
  return lock === undefined ? setField : (ignoreField as unknown as F);
}

function ignoreField(): void {}
