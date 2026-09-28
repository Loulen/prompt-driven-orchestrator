import type { UpdateSettingsRequest } from "../../shared/generated/UpdateSettingsRequest";
import { request } from "../../shared/http/client";
import type { InstanceSettings } from "../model/settings";

/** The instance settings page: per knob, the effective value and every tier (ADR-0015). */
export function fetchSettings(): Promise<InstanceSettings> {
  return request<InstanceSettings>("GET", "/settings");
}

/**
 * Persist one or more instance-config knobs and return the recomputed view
 * (#129, ADR-0015). Surfaces the daemon's fail-fast validation error (`400`)
 * verbatim so the modal can show it.
 */
export function updateSettings(
  patch: UpdateSettingsRequest,
): Promise<InstanceSettings> {
  return request<InstanceSettings>("PUT", "/settings", { body: patch });
}
