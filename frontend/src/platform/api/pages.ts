/**
 * The HTTP client of the mounted pages: `GET /pages` lists every mount with its
 * reading guide.
 */

import type { PageMount } from "../../shared/generated/PageMount";
import { request } from "../../shared/http/client";

export function fetchPageMounts(): Promise<PageMount[]> {
  return request<PageMount[]>("GET", "/pages", { label: "GET /pages" });
}
