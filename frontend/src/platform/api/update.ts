/**
 * The HTTP client of the platform context's version check and update (#697,
 * #698, #699), over the shared {@link request}.
 */

import type { UpdateApplyResponse, UpdateStatus } from "../model/update";
import type { UpdateChangelog } from "../../shared/generated/UpdateChangelog";
import { ApiError, request } from "../../shared/http/client";

/**
 * Version check (#697). `GET /update` reads the daemon's cache (zero egress);
 * `POST /update/check` forces one request to the release source and answers the
 * refreshed state — 502 with `error` + the refreshed state when the source is down,
 * 409 when the check is off or already in flight.
 */
export function fetchUpdateStatus(): Promise<UpdateStatus> {
  return request<UpdateStatus>("GET", "/update");
}

export function checkForUpdateNow(): Promise<UpdateStatus> {
  return request<UpdateStatus>("POST", "/update/check", { label: "POST /update/check" });
}

/**
 * « What's new » (#698): the release notes of every version strictly newer than the
 * installed one, newest first — or the embedded `CHANGELOG.md` with an explicit
 * `fallback_reason` when the release list is unavailable. Always answers 200.
 */
export function fetchUpdateChangelog(): Promise<UpdateChangelog> {
  return request<UpdateChangelog>("GET", "/update/changelog");
}

/**
 * Update (#699): `POST /update/apply` spawns the detached executor and answers 202 at
 * once with the attempt id — 409 with `error` when the install method is unknown or
 * an attempt is already running. `GET /update/attempts/{id}/log` is the journal, text.
 */
export function applyUpdate(): Promise<UpdateApplyResponse> {
  return request<UpdateApplyResponse>("POST", "/update/apply", { label: "POST /update/apply" });
}

export async function fetchUpdateAttemptLog(attemptId: string): Promise<string> {
  const res = await fetch(`/update/attempts/${encodeURIComponent(attemptId)}/log`);
  if (!res.ok) {
    throw new ApiError(`GET /update/attempts/${attemptId}/log failed: ${res.status}`, {
      status: res.status,
    });
  }
  return res.text();
}
