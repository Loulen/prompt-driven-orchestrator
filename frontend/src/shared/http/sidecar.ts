/**
 * The answer of a route serving a skills sidecar zip (#673): `null` on 204
 * (nothing to ship), the zip otherwise, an `ApiError` on failure.
 */

import { ApiError, apiErrorMessage } from "./client";

export async function sidecarBlob(resp: Response, label: string): Promise<Blob | null> {
  if (resp.status === 204) return null;
  if (!resp.ok) {
    const errBody = await resp.json().catch(() => null);
    throw new ApiError(apiErrorMessage(errBody, `${label} failed: ${resp.status}`), {
      status: resp.status,
      body: errBody,
    });
  }
  return resp.blob();
}

