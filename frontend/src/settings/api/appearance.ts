import type { ActorAppearance } from "../../shared/generated/ActorAppearance";
import type { AppearanceDocument } from "../../shared/generated/AppearanceDocument";
import { request } from "../../shared/http/client";

/** The caller's Apparence; `appearance: null` while it never wrote one. */
export function fetchAppearance(): Promise<ActorAppearance> {
  return request<ActorAppearance>("GET", "/actors/me/appearance");
}

/** Replace the caller's Apparence. */
export function saveAppearance(document: AppearanceDocument): Promise<ActorAppearance> {
  return request<ActorAppearance>("PUT", "/actors/me/appearance", { body: document });
}
