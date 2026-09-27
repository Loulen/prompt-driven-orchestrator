/**
 * A node's completion, as the operator drives it from the node panel: the
 * Pipeline Manager's `mark_node_done` command, and the release of an
 * interactive node's completion. A refusal is a verdict, not an exception.
 */

import type { FrontmatterViolation } from "../../shared/generated/FrontmatterViolation";
import { ApiError, apiErrorMessage, request } from "../../shared/http/client";

/** Refusal slugs this client knows how to phrase (#490, ADR-0035 §3). */
export type MarkNodeDoneRefusal =
  | "missing_outputs"
  | "frontmatter_retry_pending"
  | "frontmatter_retry_exhausted"
  | "script_validation_failed"
  | "delivery_failed"
  | "merge_conflict"
  | "merge_resolution_failed"
  | "completion_rejected";

const KNOWN_REFUSALS: readonly string[] = [
  "missing_outputs",
  "frontmatter_retry_pending",
  "frontmatter_retry_exhausted",
  "script_validation_failed",
  "delivery_failed",
  "merge_conflict",
  "merge_resolution_failed",
  "completion_rejected",
];

/**
 * What one *Mark complete* click actually did (#490, ADR-0035).
 *
 * A discriminated union on the refusal **slug**, never on the status: a status has
 * nowhere near enough bits for nine causes, and the pre-#490 code — which read
 * *every* `409` as `missing_outputs` — is the demonstration. The transition guard's
 * `409` ("resume the run first") arrived with an empty `missing` list and the banner
 * was gated on `length > 0`, so the most frequent refusal of all displayed nothing.
 *
 * A refusal is a **verdict, not an exception** (same contract as `fireTrigger`): it
 * resolves. Only a breakdown of the call itself throws.
 */
export type MarkNodeDoneOutcome =
  | { kind: "completed" }
  | { kind: "noop"; reason: string }
  | {
      kind: "refused";
      /** `null` = a slug this client does not know: render `message` as-is (ADR-0001). */
      slug: MarkNodeDoneRefusal | null;
      /** `null` = the daemon sent no `recoverable` flag; do not guess. */
      recoverable: boolean | null;
      message: string;
      missing: string[];
      violations: FrontmatterViolation[];
      body: unknown;
    };

function asStringArray(v: unknown): string[] {
  return Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : [];
}

function asViolations(v: unknown): FrontmatterViolation[] {
  if (!Array.isArray(v)) return [];
  return v.flatMap((raw) => {
    const o = raw as { port?: unknown; field?: unknown; reason?: unknown } | null;
    if (typeof o?.port !== "string" || typeof o?.field !== "string") return [];
    return [{ port: o.port, field: o.field, reason: String(o.reason ?? "") }];
  });
}

export async function markNodeDone(
  runId: string,
  nodeId: string,
  iter: number,
): Promise<MarkNodeDoneOutcome> {
  // Status-inspecting: a 409 is a refusal verdict, not a failed call, so raw mode
  // keeps the bespoke branch.
  const resp = await request<Response>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/commands`,
    { body: { kind: "mark_node_done", node_id: nodeId, iter }, responseMode: "raw" },
  );
  // GUARDED: the success body of the sibling `POST …/done` route is the bare text
  // `ok`, and the pre-#490 code threw a naked `SyntaxError` on any non-JSON body —
  // breaking this module's "one error type" contract.
  const body: unknown = await resp.json().catch(() => null);

  // 409 and 409 alone is the refusal status. A 410 (forgotten run), a 404 or a 5xx
  // are a breakdown of the call, so they stay an ApiError.
  if (resp.status === 409) {
    const b = body as
      | { error?: unknown; recoverable?: unknown; missing?: unknown; violations?: unknown; detail?: unknown }
      | null;
    const slug = typeof b?.error === "string" ? b.error : null;
    const detail = b?.detail as { missing?: unknown; violations?: unknown } | undefined;
    return {
      kind: "refused",
      slug: slug !== null && KNOWN_REFUSALS.includes(slug) ? (slug as MarkNodeDoneRefusal) : null,
      recoverable: typeof b?.recoverable === "boolean" ? b.recoverable : null,
      // Reuses the module's single message assembler (`body.message ?? body.error ??
      // fallback`), which is where the guard's prose now lands.
      message: apiErrorMessage(body, `mark_node_done refused: ${resp.status}`),
      // Reads both shapes of the evidence — flat, and the `script` fail-fast's
      // nested `detail` (ADR-0035 §5).
      missing: asStringArray(b?.missing ?? detail?.missing),
      violations: asViolations(b?.violations ?? detail?.violations),
      body,
    };
  }

  if (!resp.ok) {
    throw new ApiError(apiErrorMessage(body, `mark_node_done failed: ${resp.status}`), {
      status: resp.status,
      body,
    });
  }

  const ok = body as { noop?: unknown; reason?: unknown; status?: unknown } | null;
  if (ok?.noop === true) {
    return {
      kind: "noop",
      reason: typeof ok.reason === "string" ? ok.reason : "the node iteration was already terminal",
    };
  }
  // A `2xx` still carrying a `status` key is a pre-#490 daemon answering a refusal
  // with a success status. Never render that as a completion — that is the whole bug.
  if (typeof ok?.status === "string") {
    return {
      kind: "refused",
      slug: KNOWN_REFUSALS.includes(ok.status) ? (ok.status as MarkNodeDoneRefusal) : null,
      recoverable: null,
      message: ok.status,
      missing: [],
      violations: [],
      body,
    };
  }
  return { kind: "completed" };
}

export type ReleaseNodeCompletionOutcome =
  | { kind: "released" }
  | {
      kind: "refused";
      slug: string | null;
      recoverable: boolean | null;
      message: string;
    };

export async function releaseNodeCompletion(
  runId: string,
  nodeId: string,
  iter: number,
): Promise<ReleaseNodeCompletionOutcome> {
  const resp = await request<Response>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/nodes/${encodeURIComponent(nodeId)}/release`,
    { body: { iter }, responseMode: "raw" },
  );
  const body: unknown = await resp.json().catch(() => null);
  if (resp.status === 409) {
    const refusal = body as
      | { error?: unknown; recoverable?: unknown; message?: unknown }
      | null;
    return {
      kind: "refused",
      slug: typeof refusal?.error === "string" ? refusal.error : null,
      recoverable:
        typeof refusal?.recoverable === "boolean" ? refusal.recoverable : null,
      message: apiErrorMessage(body, `release refused: ${resp.status}`),
    };
  }
  if (!resp.ok) {
    throw new ApiError(
      apiErrorMessage(body, `release completion failed: ${resp.status}`),
      { status: resp.status, body },
    );
  }
  return { kind: "released" };
}
