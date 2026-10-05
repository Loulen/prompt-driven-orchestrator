/**
 * The HTTP client of the UI: the one seam every call to the daemon goes
 * through ({@link request}), and the one error it throws ({@link ApiError}).
 * The typed wrappers of each context sit on top of it.
 */

import { followSignInOf } from "./signIn";

/** Same-origin: the daemon serves the UI, so every path is relative to it. */
export const BASE = "";

/**
 * The one error contract for the whole client. Every non-ok response funnels
 * through {@link request} and is thrown as an `ApiError` — never a bare `Error`
 * and never a plain object. Subclassing `Error` is load-bearing: ~7 UI callers
 * render failures via `err instanceof Error ? err.message : fallback`, so a
 * plain-object contract would surface `[object Object]`.
 *
 * - `status` — HTTP status; `undefined` for a network/parse failure.
 * - `line`   — YAML validation line, lifted from a structured save-error body
 *              (`PUT /pipelines/{id}` / `PUT /runs/{id}/pipeline`); drives the
 *              SaveErrorModal `line N:` and the info-panel scroll-to-line.
 * - `body`   — the parsed JSON error body (or `null`); additive, no current
 *              reader, kept for truthful surfacing (ADR-0025).
 */
export class ApiError extends Error {
  readonly status?: number;
  readonly line?: number;
  readonly body?: unknown;
  constructor(
    message: string,
    opts: { status?: number; line?: number; body?: unknown } = {},
  ) {
    super(message);
    this.name = "ApiError";
    this.status = opts.status;
    this.line = opts.line;
    this.body = opts.body;
  }
}

/**
 * Assemble the human-readable message from a daemon error body. Mirrors the old
 * `throwStructuredSaveError`/`errorBodyMessage` idioms in one place:
 * `body.message ?? body.error ?? fallback`, with any mid-run mutation-rejection
 * reasons (409, ADR-0007 / #211) folded in from `rejections[].reason`.
 */
export function apiErrorMessage(body: unknown, fallback: string): string {
  const b = body as { message?: unknown; error?: unknown; rejections?: unknown } | null;
  let message: string;
  if (typeof b?.message === "string") message = b.message;
  else if (typeof b?.error === "string") message = b.error;
  else message = fallback;
  if (Array.isArray(b?.rejections)) {
    const reasons = (b.rejections as unknown[])
      .map((r) => (r as { reason?: unknown })?.reason)
      .filter((r): r is string => typeof r === "string");
    if (reasons.length > 0) message = `${message}: ${reasons.join("; ")}`;
  }
  return message;
}

/** The bytes an upload carries: the size of every `File`/`Blob` part of the form (#839). */
export function formDataBytes(form: FormData): number {
  let total = 0;
  form.forEach((value) => {
    if (value instanceof Blob) total += value.size;
  });
  return total;
}

/**
 * The bounded wait of an upload (#839): a minute, plus ten seconds per MB of
 * attachments. Generous for a slow link, finite for a stalled one — the same
 * rule the CLI applies to `pdo run create --file`.
 */
export function uploadTimeoutMs(uploadBytes: number): number {
  const mb = Math.ceil(uploadBytes / (1024 * 1024));
  return (60 + 10 * mb) * 1000;
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** The host the UI talks to — `BASE` is same-origin, so the page's own host. */
function daemonHost(): string {
  if (BASE) return BASE;
  if (typeof window !== "undefined" && window.location?.host) return window.location.host;
  return "the daemon";
}

const REMOTE_HINT =
  "On a remote instance, check the reverse proxy body limit (nginx `client_max_body_size`) " +
  "and `max_attachments_mb` in Settings.";

/**
 * The sentence for a request that got no answer at all (#839): a `fetch`
 * rejection, i.e. the connection was cut, refused, or the upload wait expired.
 * Nothing here came from the daemon — the message says which layer failed,
 * what was in flight and for how long.
 */
export function transportFailureMessage(args: {
  label: string;
  uploadBytes: number;
  elapsedMs: number;
  timedOut: boolean;
  cause: unknown;
}): string {
  const { label, uploadBytes, elapsedMs, timedOut, cause } = args;
  const host = daemonHost();
  const seconds = Math.max(1, Math.round(elapsedMs / 1000));
  const causeText = cause instanceof Error ? cause.message : String(cause);
  if (uploadBytes > 0) {
    const sent = formatBytes(uploadBytes);
    if (timedOut) {
      return (
        `Upload timed out after ${seconds}s: ${host} did not answer while ${sent} of ` +
        `attachments were being sent. ${REMOTE_HINT}`
      );
    }
    return (
      `Upload interrupted after ${seconds}s while sending ${sent} of attachments to ${host}: ` +
      `the network or a proxy in front of PDO cut the connection (${causeText}). ${REMOTE_HINT}`
    );
  }
  return `${label} failed: could not reach ${host} (${causeText}).`;
}

/**
 * The fallback for a non-2xx answer whose body is NOT JSON (#839): the daemon
 * always answers `{ error }`, so this came from a reverse proxy or another
 * intermediary. A 413 there is that layer's body limit, not PDO's budget.
 */
export function intermediaryFailureMessage(
  label: string,
  status: number,
  uploadBytes: number,
): string {
  const host = daemonHost();
  if (status === 413) {
    const sent = uploadBytes > 0 ? ` (${formatBytes(uploadBytes)} of attachments)` : "";
    return (
      `The server in front of PDO at ${host} refused the request body${sent} before it ` +
      `reached the daemon (413). No PDO error came back, so this is the reverse proxy's ` +
      "limit (nginx `client_max_body_size`), not `max_attachments_mb`: raise it or attach less."
    );
  }
  return (
    `${label} failed: ${status} (no PDO error body — the reply came from a proxy or ` +
    `another intermediary in front of ${host}).`
  );
}

/**
 * How {@link request} turns a 2xx response into its resolved value:
 * - `json` (default) — `await resp.json()`
 * - `text`           — `await resp.text()` (prompts, artifacts, diffs)
 * - `void`           — resolve `undefined` without touching the body (commands)
 * - `raw`            — resolve the `Response` itself; the caller inspects
 *                      `status`/`ok` and does its own body read. The single
 *                      escape hatch for the wrappers with bespoke status logic.
 */
export type ResponseMode = "json" | "text" | "void" | "raw";

export interface RequestOpts {
  /** `object` → JSON body + `Content-Type: application/json`; `FormData` → sent
   *  as-is so the browser sets the multipart boundary; `Blob` → sent as-is with
   *  its own type (a raw text save); `undefined` → no body. */
  body?: unknown;
  /** Query params appended to `path`; `undefined` values are dropped, keys and
   *  values are `encodeURIComponent`-encoded. */
  query?: Record<string, string | number | boolean | undefined>;
  /** Response handling; defaults to `"json"`. */
  responseMode?: ResponseMode;
  /** Fallback error label; defaults to `` `${method} ${path}` ``. */
  label?: string;
  /**
   * Let the request outlive the document (#594). A normal `fetch` started while
   * the page is unloading is cancelled with it, so the assistant's reap on
   * `pagehide` would never leave the browser. `sendBeacon` is not an option: it
   * is POST-only and this is a `DELETE`. Fire-and-forget by nature — the
   * response is not guaranteed to arrive.
   */
  keepalive?: boolean;
}

/**
 * The single HTTP seam. Owns `BASE`, URL + query building, headers, body
 * encoding, response parsing, and error construction. Every exported wrapper is
 * a thin typed call over this; on a non-ok response (outside `raw` mode) it
 * throws one {@link ApiError} carrying `status`, `line`, and the parsed `body`.
 */
export async function request<T = unknown>(
  method: string,
  path: string,
  opts: RequestOpts = {},
): Promise<T> {
  const { body, query, responseMode = "json", label, keepalive } = opts;

  let url = BASE + path;
  if (query) {
    const qs = Object.entries(query)
      .filter(([, v]) => v !== undefined)
      .map(([k, v]) => `${encodeURIComponent(k)}=${encodeURIComponent(String(v))}`)
      .join("&");
    if (qs) url += (path.includes("?") ? "&" : "?") + qs;
  }

  const init: RequestInit = { method };
  if (keepalive) init.keepalive = true;
  // #507: declare the request's origin on EVERY call (falsifiable hint read
  // into `audit_log.actor_hint`, never a gate). Set unconditionally — the JSON
  // branch alone would miss FormData and body-less GET/DELETE.
  const headers: Record<string, string> = { "X-PDO-Actor": "ui" };
  if (body instanceof FormData) {
    init.body = body; // browser sets the multipart boundary — no Content-Type
  } else if (body instanceof Blob) {
    // A raw body (a plain-text file save, #671): sent as-is, the Blob's own type.
    init.body = body;
    if (body.type) headers["Content-Type"] = body.type;
  } else if (body !== undefined) {
    headers["Content-Type"] = "application/json";
    init.body = JSON.stringify(body);
  }
  init.headers = headers;
  const fallbackLabel = label ?? `${method} ${path}`;

  // #839: an upload (a multipart body) gets a bounded wait that grows with
  // its size — a stalled transfer expires with a sentence instead of leaving
  // the modal spinning forever. Other requests keep the browser's own limits.
  const uploadBytes = body instanceof FormData ? formDataBytes(body) : 0;
  let controller: AbortController | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  if (body instanceof FormData) {
    controller = new AbortController();
    init.signal = controller.signal;
    timer = setTimeout(() => controller?.abort(), uploadTimeoutMs(uploadBytes));
  }
  const startedAt = Date.now();

  let resp: Response;
  try {
    resp = await fetch(url, init);
  } catch (e) {
    // The request never got an answer: the network or a proxy in front of
    // the daemon cut it, or the upload wait above expired. The browser's raw
    // `TypeError: Failed to fetch` says none of that — name the layer, the
    // host, the payload and the elapsed time (#839).
    throw new ApiError(
      transportFailureMessage({
        label: fallbackLabel,
        uploadBytes,
        elapsedMs: Date.now() - startedAt,
        timedOut: controller?.signal.aborted === true,
        cause: e,
      }),
      { body: null },
    );
  } finally {
    if (timer !== undefined) clearTimeout(timer);
  }
  if (responseMode === "raw") return resp as unknown as T; // caller owns status

  if (!resp.ok) {
    // `undefined` ⇔ the body is not JSON at all — a proxy's HTML error page,
    // never the daemon, whose refusals always carry `{ error }` (#839).
    const errBody: unknown = await resp.json().catch(() => undefined);
    // 0.8: an extension refused a request nobody identified, naming its
    // sign-in page: go there (once), the call still fails below.
    if (resp.status === 401) followSignInOf(errBody);
    const line =
      typeof (errBody as { line?: unknown } | null)?.line === "number"
        ? (errBody as { line: number }).line
        : undefined;
    const fallback =
      errBody === undefined
        ? intermediaryFailureMessage(fallbackLabel, resp.status, uploadBytes)
        : `${fallbackLabel} failed: ${resp.status}`;
    throw new ApiError(apiErrorMessage(errBody, fallback), {
      status: resp.status,
      line,
      body: errBody ?? null,
    });
  }
  if (responseMode === "void") return undefined as T;
  if (responseMode === "text") return (await resp.text()) as unknown as T;
  return (await resp.json()) as T;
}
