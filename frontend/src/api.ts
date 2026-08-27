import type { PipelineListEntry, PipelineDetail, PipelineDef, RunListEntry, RunState, PortDef, PortSide, PortType, FrontmatterFieldDecl, FrontmatterViolation, Trigger, TriggerFire, DaemonStatus, InstanceSettings, UpdateSettingsRequest, StatsOverview, StatsCost, SandboxProfile, SandboxProfileImage, SandboxProfileReferents, SyncCostPricesReport, Project, BranchRef } from "./types";
import { foldHarnessOntoNode } from "./lib/harness";

const BASE = "";

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
function apiErrorMessage(body: unknown, fallback: string): string {
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
   *  as-is so the browser sets the multipart boundary; `undefined` → no body. */
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
  } else if (body !== undefined) {
    headers["Content-Type"] = "application/json";
    init.body = JSON.stringify(body);
  }
  init.headers = headers;

  const resp = await fetch(url, init);
  if (responseMode === "raw") return resp as unknown as T; // caller owns status

  if (!resp.ok) {
    const errBody = await resp.json().catch(() => null);
    const line =
      typeof (errBody as { line?: unknown } | null)?.line === "number"
        ? (errBody as { line: number }).line
        : undefined;
    throw new ApiError(
      apiErrorMessage(errBody, `${label ?? `${method} ${path}`} failed: ${resp.status}`),
      { status: resp.status, line, body: errBody },
    );
  }
  if (responseMode === "void") return undefined as T;
  if (responseMode === "text") return (await resp.text()) as unknown as T;
  return (await resp.json()) as T;
}

export function fetchRuns(): Promise<RunListEntry[]> {
  return request<RunListEntry[]>("GET", "/runs");
}

export function fetchSessions(): Promise<DaemonStatus> {
  return request<DaemonStatus>("GET", "/sessions");
}

/** Instance-wide settings, per knob (#129, ADR-0015). */
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

// --- Staging profiles (#432, ADR-0031 §2-§7) --------------------------------
//
// A separate REST resource, NOT part of the grouped `PUT /settings`: a profile is a ROW,
// not a `{effective, source, stored, env, default}` knob. That is exactly why the editor's
// footer says "Done" rather than "Save" — nothing is batched behind it.
//
// The routes sit under `/settings/…` purely for ROUTING: the vite proxy key is a prefix,
// so `'/settings'` already covers every sub-path (no proxy edit, none of the "dev GET
// answers 200 with the SPA" traps that `/nodes`, `/stats` and `/fs` each paid).

/** Every staging profile, each fully resolved (`+ home`, the host `$HOME`). */
export function fetchSandboxProfiles(): Promise<{
  profiles: SandboxProfile[];
  home: string | null;
}> {
  return request("GET", "/settings/sandbox-profiles");
}

/** One resolved profile; throws `ApiError` with status 404 when the name is unknown. */
export function fetchSandboxProfile(name: string): Promise<SandboxProfile> {
  return request<SandboxProfile>(
    "GET",
    `/settings/sandbox-profiles/${encodeURIComponent(name)}`,
  );
}

/**
 * Upsert a profile's **diff** — `disabled` / `extras`, never a snapshot (ADR-0031 §2) —
 * plus its `env` map (#468, ADR-0031 §8) and its `image` source (#467, ADR-0031 §9), neither
 * of which is a diff at all.
 * Upsert, because the caller cannot know whether `full` already has a row, and editing it
 * IS what materialises one. Returns the recomputed view so the editor needs no refetch.
 *
 * Every field is a FULL replacement, `env` and `image` included: omitting a variable is how you
 * remove it, and `image: null` is how you go back to the instance-wide setting. So every caller
 * passes the fields it is NOT changing verbatim — which is why the editor threads them through one
 * `write` helper rather than per-control.
 */
export function saveSandboxProfile(
  name: string,
  diff: {
    disabled: string[];
    extras: string[];
    env: Record<string, string>;
    image: SandboxProfileImage | null;
  },
): Promise<SandboxProfile> {
  return request<SandboxProfile>(
    "PUT",
    `/settings/sandbox-profiles/${encodeURIComponent(name)}`,
    { body: diff },
  );
}

/**
 * Delete the materialised row. Unconditional (ADR-0031 §7 — a *soft* guard-rail, no
 * referential integrity in the DB): deleting an edited `full`/`minimal` reverts it to its
 * virtual default, deleting a user profile makes its referents' next Run fail loud.
 * Neither repoints anything, which is what {@link fetchSandboxProfileReferents} is for.
 */
export function deleteSandboxProfile(name: string): Promise<void> {
  return request<void>(
    "DELETE",
    `/settings/sandbox-profiles/${encodeURIComponent(name)}`,
    { responseMode: "void" },
  );
}

/** Who still points at a profile — server-side, because `RunListEntry` carries no `sandbox`. */
export function fetchSandboxProfileReferents(
  name: string,
): Promise<SandboxProfileReferents> {
  return request<SandboxProfileReferents>(
    "GET",
    `/settings/sandbox-profiles/${encodeURIComponent(name)}/referents`,
  );
}

/**
 * Fetch the remote price source and rewrite the fetched price tier (#427,
 * ADR-0034). The daemon's only outbound call from a user gesture.
 *
 * Under `/settings/…` deliberately: the vite dev proxy keys on a PREFIX, so this
 * needs no `vite.config.ts` line — the trap `/nodes` (#345), `/stats` (#377) and
 * `/fs` (#431) each paid, where a missing proxy entry makes a dev-mode request
 * answer 200 with the SPA.
 *
 * Throws on 409 (a sync is already in flight) and on 502 (source unreachable, or
 * an empty harvest — in which case nothing was written and the last known table
 * survives). A run with nothing to change resolves with `noop: true`.
 */
export function syncCostPrices(): Promise<SyncCostPricesReport> {
  return request<SyncCostPricesReport>("POST", "/settings/cost-prices/sync", {
    label: "POST /settings/cost-prices/sync",
  });
}

/**
 * Cheap instance stats over `[from, to)` bucketed by `bucket` (#377): runs,
 * errors (`run_failed`), sessions, fires-per-pipeline, and the "triggers that
 * created a run" KPI. Indexed SQL — safe to fetch on modal open.
 */
export function fetchStatsOverview(
  from: string,
  to: string,
  bucket: string,
): Promise<StatsOverview> {
  return request<StatsOverview>("GET", "/stats/overview", { query: { from, to, bucket } });
}

/**
 * Estimated cost over `[from, to)`, folded by period/pipeline/project (#377,
 * ADR-0022/0029). Heavy (memoized per-run cost fanned over the window) — fetch
 * lazily, only when the cost tab is shown.
 */
export function fetchStatsCost(
  from: string,
  to: string,
  bucket: string,
): Promise<StatsCost> {
  return request<StatsCost>("GET", "/stats/cost", { query: { from, to, bucket } });
}

export function fetchRun(runId: string): Promise<RunState> {
  return request<RunState>("GET", `/runs/${encodeURIComponent(runId)}`);
}

export function fetchRunEvents(runId: string): Promise<unknown[]> {
  return request<unknown[]>("GET", `/runs/${encodeURIComponent(runId)}/events`);
}

/** Refusal slugs this client knows how to phrase (#490, ADR-0035 §3). */
export type MarkNodeDoneRefusal =
  | "missing_outputs"
  | "frontmatter_retry_pending"
  | "frontmatter_retry_exhausted"
  | "script_validation_failed"
  | "doc_violated_code_immutability"
  | "merge_conflict"
  | "merge_resolution_failed"
  | "completion_rejected";

const KNOWN_REFUSALS: readonly string[] = [
  "missing_outputs",
  "frontmatter_retry_pending",
  "frontmatter_retry_exhausted",
  "script_validation_failed",
  "doc_violated_code_immutability",
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

export function attachSession(sessionId: string): Promise<void> {
  return request<void>(
    "POST",
    `/sessions/${encodeURIComponent(sessionId)}/attach`,
    { responseMode: "void", label: "attach" },
  );
}

export function attachManager(runId: string): Promise<void> {
  return request<void>(
    "POST",
    `/sessions/${encodeURIComponent(runId)}/manager/attach`,
    { responseMode: "void", label: "manager attach" },
  );
}

/**
 * Open (or re-attach) an ad-hoc bash shell in a terminal run's pipeline
 * worktree (#316 / ADR-0021). Create-if-absent; returns the tmux session name to
 * attach to via the existing `WS /sessions/<session>/pty` bridge (no OS spawn).
 * `created` distinguishes a fresh shell from a re-attach.
 */
export function openRunShell(
  runId: string,
): Promise<{ session: string; created: boolean }> {
  return request<{ session: string; created: boolean }>(
    "POST",
    `/sessions/${encodeURIComponent(runId)}/shell`,
    { label: "open shell" },
  );
}

/**
 * Open (or re-attach) the library pipeline authoring assistant (#302 / ADR-0048,
 * #594 / ADR-0051). Create-if-absent; returns the shared tmux session name to
 * attach to via the existing `WS /sessions/<session>/pty` bridge.
 *
 * No pipeline id: there is **one** assistant for the whole daemon, and which
 * template it works on travels through {@link putLibassistFocus} instead.
 */
export function openLibraryAssistant(): Promise<{
  session: string;
  created: boolean;
}> {
  return request<{ session: string; created: boolean }>(
    "POST",
    "/sessions/libassist",
    { label: "open assistant" },
  );
}

/**
 * Reap the library authoring assistant (#302 / ADR-0048, #594 / ADR-0051).
 *
 * Called when the user leaves **every** pipeline edit view — not when they leave
 * the Assistant tab, which used to throw the conversation away on each round trip
 * between two templates. Best-effort: reaping an absent session is a no-op.
 *
 * `keepalive` lets the request survive the document, which is the only way the
 * `pagehide` path can send anything at all.
 */
export function closeLibraryAssistant(
  opts: { keepalive?: boolean } = {},
): Promise<{ ok: boolean; reaped: boolean }> {
  return request<{ ok: boolean; reaped: boolean }>(
    "DELETE",
    "/sessions/libassist",
    { label: "close assistant", keepalive: opts.keepalive },
  );
}

/**
 * Declare which template the UI is editing — or `null` to clear it (#594 /
 * ADR-0051). Sent on every edit-view change and repeated as a heartbeat.
 *
 * Two jobs in one call: it is how the assistant learns the open pipeline on its
 * next message, and it is how the daemon's reaper knows a human is still editing
 * even when no terminal is attached. Only `{id, scope}` — the daemon resolves the
 * file path, because "scope" does not mean the same thing on an edit tab as it
 * does in the library store.
 */
export function putLibassistFocus(
  pipelineId: string | null,
  scope?: string,
): Promise<unknown> {
  return request("PUT", "/sessions/libassist/focus", {
    body: { pipeline_id: pipelineId, scope },
    label: "declare assistant focus",
  });
}

export interface PaneResponse {
  content: string;
  session_name: string;
  resumed: boolean;
  stale: boolean;
  /**
   * Provenance of `content` (#205): "live" (captured from a running session),
   * "resumed" (a dead latest-iter session was re-attached), "snapshot" (the
   * persisted post-mortem pane of a reaped terminal node), or "unavailable"
   * (no session and no snapshot).
   */
  source: "live" | "resumed" | "snapshot" | "unavailable";
}

export function fetchPrompt(
  runId: string,
  nodeId: string,
  iter: number,
): Promise<string> {
  return request<string>(
    "GET",
    `/runs/${encodeURIComponent(runId)}/nodes/${encodeURIComponent(nodeId)}/prompt`,
    { query: { iter }, responseMode: "text", label: "GET prompt" },
  );
}

// --- Node IO ---

export interface FileInfo {
  path: string;
  exists: boolean;
  size: number | null;
  frontmatter: Record<string, unknown> | null;
}

export interface PortIO {
  port: string;
  repeated: boolean;
  port_type?: PortType;
  files: FileInfo[];
}

export interface NodeIO {
  inputs: PortIO[];
  outputs: PortIO[];
}

export function fetchNodeIO(
  runId: string,
  nodeId: string,
  iter: number,
): Promise<NodeIO> {
  return request<NodeIO>(
    "GET",
    `/runs/${encodeURIComponent(runId)}/nodes/${encodeURIComponent(nodeId)}/io`,
    { query: { iter }, label: "GET io" },
  );
}

export function fetchArtifact(
  runId: string,
  relativePath: string,
): Promise<string> {
  return request<string>(
    "GET",
    `/runs/${encodeURIComponent(runId)}/artifact`,
    { query: { path: relativePath }, responseMode: "text", label: "GET artifact" },
  );
}

export function artifactUrl(runId: string, relativePath: string): string {
  return `${BASE}/runs/${encodeURIComponent(runId)}/artifact?path=${encodeURIComponent(relativePath)}`;
}

export function fetchPane(
  runId: string,
  nodeId: string,
  iter: number,
): Promise<PaneResponse> {
  return request<PaneResponse>(
    "GET",
    `/runs/${encodeURIComponent(runId)}/nodes/${encodeURIComponent(nodeId)}/pane`,
    { query: { iter }, label: "GET pane" },
  );
}

export function fetchPipelines(): Promise<PipelineListEntry[]> {
  return request<PipelineListEntry[]>("GET", "/pipelines");
}

/** One repo line of a multi-repo create (#465, ADR-0042/0047): a path, an optional
 *  base branch (default HEAD, the local ref), and an optional `read_only` opt-in
 *  (default `false` ⇒ writable). `[0]` is the primary; `[1..]` become secondary
 *  snapshots, writable unless `read_only`. Shared by create, `EditRunReposBody.add`
 *  and the trigger `target_repos` blob. */
export interface TargetRepoInput {
  repo: string;
  base_branch?: string;
  read_only?: boolean;
}

export interface CreateRunRequest {
  pipeline: string;
  input: string;
  variables: Record<string, unknown>;
  pipeline_id?: string;
  target_repo?: string;
  /** Read-only secondary repos (#465). `[0]` is the primary (kept in sync with
   *  `target_repo`), `[1..]` are secondaries. Omit for a mono-repo Run. */
  target_repos?: TargetRepoInput[];
  source_branch?: string;
  name?: string;
  /** Explicit sandbox (#410/#432): `"off"` or a staging-profile name. Omitted → the
   *  server defers to the trigger/instance default at the create chokepoint. */
  sandbox?: string;
  /** Explicit harness (#551, ADR-0046): the `run` tier of the precedence chain. Omitted/
   *  blank → the Run names no harness and each free node resolves through the instance
   *  default and the `claude` floor. Frozen into `RunStarted` at the create chokepoint. */
  harness?: string;
  /** Whether the manager auto-names this Run (#338). The modal always sends it; omit and
   *  the server resolves back-compat by the presence of `name`, then the instance default. */
  auto_name?: boolean;
  images?: File[];
}

export interface CreateRunResponse {
  run_id: string;
}

export function createRun(req: CreateRunRequest): Promise<CreateRunResponse> {
  const hasImages = req.images && req.images.length > 0;

  if (hasImages) {
    const form = new FormData();
    form.append("pipeline", req.pipeline);
    form.append("input", req.input);
    form.append("variables", JSON.stringify(req.variables));
    if (req.pipeline_id) form.append("pipeline_id", req.pipeline_id);
    if (req.target_repo) form.append("target_repo", req.target_repo);
    // #465: the multi-repo list rides the multipart create as a JSON field (mirrors
    // `variables`), so a Run created WITH attached images keeps its secondaries.
    if (req.target_repos && req.target_repos.length > 0)
      form.append("target_repos", JSON.stringify(req.target_repos));
    if (req.source_branch) form.append("source_branch", req.source_branch);
    if (req.name) form.append("name", req.name);
    // #410: thread the explicit sandbox mode through the multipart path too, so a
    // sandboxed Run created WITH attached images keeps its mode (the daemon's
    // multipart parser reads this field).
    if (req.sandbox) form.append("sandbox", req.sandbox);
    // #551: thread the explicit harness through the multipart path too, so a Run created
    // WITH attached images keeps its harness choice (the daemon's multipart parser reads
    // this field). Omitted when blank — the Run then names no harness.
    if (req.harness) form.append("harness", req.harness);
    // #338: thread the explicit auto-naming choice through the multipart path too, so an
    // unchecked "Auto-generated" box is honoured on a create WITH attached images. Sent as
    // a stringified bool; only when the caller made a choice (it always does from the modal).
    if (req.auto_name !== undefined) form.append("auto_name", String(req.auto_name));
    for (const file of req.images!) {
      form.append("images", file, file.name);
    }
    // FormData → no manual Content-Type, so the browser sets the boundary.
    return request<CreateRunResponse>("POST", "/runs", { body: form, label: "POST /runs" });
  }
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  const { images: _omitted, ...jsonBody } = req;
  return request<CreateRunResponse>("POST", "/runs", { body: jsonBody, label: "POST /runs" });
}

/** The body of a mid-run repo-list edit (#465 slice 2). `add` entries mirror the
 *  create-modal secondary rows; `remove` names aliases. Both optional — an empty
 *  body is a legal no-op. */
export interface EditRunReposBody {
  add?: TargetRepoInput[];
  remove?: string[];
}

/**
 * What one `PATCH /runs/{id}/repos` did (#465 slice 2, ADR-0042).
 *
 * A refusal is a **verdict, not an exception** — same contract as {@link markNodeDone}:
 * the daemon names the cause with a slug and prose, so the panel shows it inline
 * rather than throwing a hard error. Only a breakdown of the call itself (network)
 * throws.
 */
export type EditRunReposOutcome =
  | { kind: "ok"; run: RunState }
  | { kind: "refused"; slug: string | null; message: string; status: number };

/**
 * Add / remove read-only secondary repos on a live Run (#465 slice 2, ADR-0042).
 *
 * Returns the reprojected {@link RunState} on success (so the caller refreshes in one
 * round-trip), or a typed refusal on any 4xx/5xx the daemon argues
 * (`run_not_editable`, `secondary_is_primary`, `bad_secondary_repo`, …). Precedent:
 * `PATCH /triggers/{id}` for the verb, {@link markNodeDone} for the verdict shape.
 */
export async function editRunRepos(
  runId: string,
  body: EditRunReposBody,
): Promise<EditRunReposOutcome> {
  // Raw mode: a 4xx/5xx here is a nameable refusal, not a failed call, so we read the
  // body ourselves instead of letting `request` throw.
  const resp = await request<Response>(
    "PATCH",
    `/runs/${encodeURIComponent(runId)}/repos`,
    { body, responseMode: "raw", label: `PATCH /runs/${runId}/repos` },
  );
  const parsed: unknown = await resp.json().catch(() => null);
  if (resp.ok) {
    return { kind: "ok", run: parsed as RunState };
  }
  const b = parsed as { error?: unknown } | null;
  return {
    kind: "refused",
    slug: typeof b?.error === "string" ? b.error : null,
    message: apiErrorMessage(parsed, `edit repos refused: ${resp.status}`),
    status: resp.status,
  };
}

// --- Triggers (#160) ---

export interface CreateTriggerRequest {
  name: string;
  pipeline_id: string;
  cron: string;
  input_template?: string;
  target_repo?: string;
  /** Read-only secondary repos to associate with fired Runs (#465). `[0]` = primary
   *  (kept in sync with `target_repo`), `[1..]` secondaries. Omit for mono-repo. */
  target_repos?: TargetRepoInput[];
  source_branch?: string;
  variables?: Record<string, unknown>;
  guard_command?: string;
  overlap_policy?: string;
  /** Bounded-`allow` ceiling (#239): max simultaneous live Runs; omit/undefined = unbounded. */
  max_concurrent?: number | null;
  /** Per-Trigger sandbox (#410/#432): `"off"` or a staging-profile name, or null/omit to
   *  inherit the instance default. */
  sandbox?: string | null;
  /** Per-Trigger harness (#551): a harness name, or null/omit to inherit the instance
   *  default. Folded into the fired Run's harness (no separate Trigger tier). */
  harness?: string | null;
  /** Whether Runs this Trigger fires are auto-named (#338). Seeded from the instance
   *  default in the modal; omit → the server defaults to `true` (pre-#338 behaviour). */
  auto_name?: boolean;
}

export function fetchTriggers(): Promise<Trigger[]> {
  return request<Trigger[]>("GET", "/triggers");
}

export function createTrigger(req: CreateTriggerRequest): Promise<Trigger> {
  return request<Trigger>("POST", "/triggers", { body: req, label: "POST /triggers" });
}

export function fetchTrigger(triggerId: string): Promise<Trigger> {
  return request<Trigger>("GET", `/triggers/${encodeURIComponent(triggerId)}`);
}

/**
 * A partial Trigger edit (#162). Omitted fields are left unchanged. `enabled`
 * toggles activation; the config fields cover schedule, input template, and
 * overlap policy (plus name/repo/branch/guard for completeness).
 */
export interface UpdateTriggerRequest {
  name?: string;
  /** Repoint the trigger to a different pipeline (#230). Validated server-side. */
  pipeline_id?: string;
  enabled?: boolean;
  cron?: string;
  input_template?: string;
  overlap_policy?: string;
  target_repo?: string | null;
  /** Read-only secondary repos (#465): an array sets the list, `null` clears to
   *  mono-repo, `undefined` leaves it unchanged. */
  target_repos?: TargetRepoInput[] | null;
  source_branch?: string | null;
  guard_command?: string | null;
  variables?: Record<string, unknown>;
  /** Bounded-`allow` ceiling (#239): number sets, null clears to unbounded, undefined leaves unchanged. */
  max_concurrent?: number | null;
  /** Per-Trigger sandbox (#410/#432): a value sets it, `null` clears back to
   *  inheriting the instance default, `undefined` leaves it unchanged. */
  sandbox?: string | null;
  /** Per-Trigger harness (#551): a value sets it, `null` clears back to inheriting the
   *  instance default, `undefined` leaves it unchanged. */
  harness?: string | null;
  /** Auto-naming toggle (#338): a bool sets it, `undefined` leaves it unchanged. A flat
   *  bool (no clear state) — mirror of `enabled`. */
  auto_name?: boolean;
}

export function updateTrigger(
  triggerId: string,
  req: UpdateTriggerRequest,
): Promise<Trigger> {
  return request<Trigger>(
    "PATCH",
    `/triggers/${encodeURIComponent(triggerId)}`,
    { body: req, label: `PATCH /triggers/${triggerId}` },
  );
}

// --- Projets (#552, ADR-0046) ---

/** All Projets with their members (`GET /projects`). */
export function fetchProjects(): Promise<Project[]> {
  return request<Project[]>("GET", "/projects");
}

/** Materialise a Projet from a bare name (`POST /projects`). */
export function createProject(name: string): Promise<Project> {
  return request<Project>("POST", "/projects", {
    body: { name },
    label: "POST /projects",
  });
}

/**
 * Rename a Projet and/or (re)set the harness it carries (`PATCH /projects/{id}`).
 * `harness`: a string sets it, `null` clears it, `undefined` leaves it unchanged
 * (double-`Option` on the wire). `name` omitted leaves it unchanged.
 */
export interface UpdateProjectRequest {
  name?: string;
  harness?: string | null;
}

export function updateProject(
  projectId: string,
  req: UpdateProjectRequest,
): Promise<Project> {
  return request<Project>("PATCH", `/projects/${encodeURIComponent(projectId)}`, {
    body: req,
    label: `PATCH /projects/${projectId}`,
  });
}

/**
 * Attach a member path to a Projet (`POST /projects/{id}/members`). Throws an
 * {@link ApiError} with `status: 409` whose message names the owning Projet when
 * the path already belongs to a different one (AC: refus nommant le propriétaire).
 */
export function addProjectMember(projectId: string, path: string): Promise<Project> {
  return request<Project>(
    "POST",
    `/projects/${encodeURIComponent(projectId)}/members`,
    { body: { path }, label: `POST /projects/${projectId}/members` },
  );
}

/** Detach a member path from a Projet (`DELETE /projects/{id}/members`). */
export function removeProjectMember(projectId: string, path: string): Promise<Project> {
  return request<Project>(
    "DELETE",
    `/projects/${encodeURIComponent(projectId)}/members`,
    { body: { path }, label: `DELETE /projects/${projectId}/members` },
  );
}

/** Delete a Projet and its memberships (`DELETE /projects/{id}`). */
export function deleteProject(projectId: string): Promise<void> {
  return request<void>("DELETE", `/projects/${encodeURIComponent(projectId)}`, {
    responseMode: "void",
    label: `DELETE /projects/${projectId}`,
  });
}

export async function deleteTrigger(triggerId: string): Promise<void> {
  // Status-inspecting: a 404 is a tolerated success (idempotent delete), so raw
  // mode keeps the bespoke guard rather than routing through the core's throw.
  const resp = await request<Response>(
    "DELETE",
    `/triggers/${encodeURIComponent(triggerId)}`,
    { responseMode: "raw" },
  );
  if (!resp.ok && resp.status !== 404) {
    throw new ApiError(`DELETE /triggers/${triggerId} failed: ${resp.status}`, { status: resp.status });
  }
}

/** Response of `POST /triggers/{id}/fire` (#341, ADR-0027). A guard/overlap
 * skip is an honest 200 with `fired: false`; disabled/dangling is a thrown 409. */
export interface FireTriggerResponse {
  ok: boolean;
  fired: boolean;
  run_id?: string | null;
  outcome?: string | null;
  reason?: string | null;
}

/** Manually fire a Trigger — a first-class fire (guard + overlap + history). */
export function fireTrigger(triggerId: string): Promise<FireTriggerResponse> {
  return request<FireTriggerResponse>(
    "POST",
    `/triggers/${encodeURIComponent(triggerId)}/fire`,
    { label: `POST /triggers/${triggerId}/fire` },
  );
}

export function fetchTriggerFires(triggerId: string): Promise<TriggerFire[]> {
  return request<TriggerFire[]>("GET", `/triggers/${encodeURIComponent(triggerId)}/fires`);
}

/**
 * #348 global Trigger kill-switch: pause (or resume) all scheduled fires
 * daemon-wide. Idempotent; returns the applied state. The per-Trigger `enabled`
 * flag is untouched — pause is an orthogonal channel — so resuming restores the
 * prior state for free. Manual "Run now" still fires while paused.
 */
export function pauseTriggers(paused: boolean): Promise<{ ok: boolean; paused: boolean }> {
  return request("POST", "/triggers/pause", {
    body: { paused },
    label: `POST /triggers/pause ${paused}`,
  });
}

/** Scheduler liveness + global pause flag (#222/#348). Hydrates the paused flag
 * on mount, since there is no trigger polling to carry it. */
export function fetchTriggersHealth(): Promise<{
  last_tick_at: string | null;
  tick_interval_secs: number;
  paused: boolean;
}> {
  return request("GET", "/triggers/health", { label: "GET /triggers/health" });
}

/** Verdict of `POST /triggers/guard/test` (#350): a 1:1 projection of the
 * backend `GuardResult`. `outcome` drives the client-side would-fire / would-skip
 * / guard-error label. */
export interface TestGuardResponse {
  outcome: "pass" | "skip" | "error";
  stdout: string;
  stderr: string;
  exit_code: number | null;
  detail: string | null;
}

/**
 * Dry-run a Trigger guard command — the opposite pole of "Run now" (ADR-0027
 * addendum, #350). Runs the guard *as currently typed* through the pure
 * `run_guard` seam with **zero side effects** (no Run, no fire history, no
 * `next_fire_at` bump) and returns the verdict. `target_repo` is optional; when
 * omitted the daemon runs the guard in its own repo_root.
 */
export function testGuard(
  guard_command: string,
  target_repo?: string,
): Promise<TestGuardResponse> {
  return request<TestGuardResponse>(
    "POST",
    "/triggers/guard/test",
    { body: { guard_command, target_repo }, label: "POST /triggers/guard/test" },
  );
}

// --- Repo validation and branch listing ---

export interface ValidateRepoResponse {
  valid: boolean;
  error?: string;
}

export async function validateRepo(path: string): Promise<ValidateRepoResponse> {
  // No `resp.ok` check by contract: the `{ valid, error }` body is authoritative
  // even on a non-2xx, so raw mode reads the body unconditionally (never throws).
  const resp = await request<Response>(
    "GET",
    `/repos/validate?path=${encodeURIComponent(path)}`,
    { responseMode: "raw" },
  );
  return resp.json();
}

export function listBranches(repoPath: string): Promise<BranchRef[]> {
  return request<BranchRef[]>(
    "GET",
    `/repos/branches?path=${encodeURIComponent(repoPath)}`,
    { label: "GET /repos/branches" },
  );
}

export function fetchRecentRepos(): Promise<string[]> {
  return request<string[]>("GET", "/repos/recent");
}

// --- Filesystem explorer (#131, generalised in #431) ---

export interface BrowseEntry {
  name: string;
  path: string;
  /** `.git`-presence hint (never a `git rev-parse`); meaningless for a file. */
  is_git_repo: boolean;
  is_symlink: boolean;
  /**
   * #431: `true` for a directory (symlinks FOLLOWED), `false` for a regular file.
   * Always emitted, including under the dirs-only default where it is invariably
   * `true`, so an entry's shape never depends on the request.
   */
  is_dir: boolean;
}

export interface BrowseResponse {
  /** The directory actually listed (canonicalized). */
  path: string;
  /** Parent directory, or null only at the filesystem root. */
  parent: string | null;
  entries: BrowseEntry[];
  /** True iff the post-filter entry count exceeded the listing cap. */
  truncated: boolean;
  /** Non-null when the dir was navigable but unlistable (e.g. permission denied). */
  error: string | null;
}

/**
 * Optional widening of the listing (#431). Both flags are **off** by default, which
 * is the pre-rename behaviour bit for bit: directories only, dot-entries filtered.
 * A flag only travels on the wire when `true`, so the default call's URL is exactly
 * `/fs/browse`, with no query string at all.
 */
export interface BrowseOptions {
  files?: boolean;
  hidden?: boolean;
}

/**
 * List `path` (or the daemon's default chain `$HOME → repo_root → /` when omitted).
 * 200 always carries the {@link BrowseResponse} shape — including the in-body
 * `error` for navigable-but-unlistable dirs — so callers branch on `data.error`.
 * Only genuine caller/system bugs (relative path → 400, collapsed default → 500)
 * throw here.
 *
 * `path` stays the FIRST positional parameter, and in default mode callers must pass
 * it as the SOLE argument: `RepoCombobox.test.tsx` pins `browseFs(undefined)` /
 * `browseFs("/abs/repo/path")` and vitest compares arity strictly (a trailing
 * `undefined` is a recorded second argument and breaks `toHaveBeenCalledWith`).
 */
export function browseFs(path?: string, opts: BrowseOptions = {}): Promise<BrowseResponse> {
  return request<BrowseResponse>("GET", "/fs/browse", {
    // `request` drops undefined-valued keys, so `|| undefined` keeps `files=false`
    // off the wire instead of sending a redundant explicit default.
    query: { path, files: opts.files || undefined, hidden: opts.hidden || undefined },
    label: "GET /fs/browse",
  });
}

export function killNode(
  runId: string,
  nodeId: string,
  iter: number,
): Promise<void> {
  return request<void>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/commands`,
    { body: { kind: "kill_node", node_id: nodeId, iter }, responseMode: "void", label: "kill_node" },
  );
}

export function restartNode(
  runId: string,
  nodeId: string,
  iter: number,
): Promise<void> {
  return request<void>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/commands`,
    { body: { kind: "restart_node", node_id: nodeId, iter }, responseMode: "void", label: "restart_node" },
  );
}

export function pauseRun(runId: string): Promise<void> {
  return request<void>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/commands`,
    { body: { kind: "pause_run" }, responseMode: "void", label: "pause_run" },
  );
}

export function resumeRun(runId: string): Promise<void> {
  return request<void>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/commands`,
    { body: { kind: "resume_run" }, responseMode: "void", label: "resume_run" },
  );
}

/**
 * The global re-open (#598 / ADR-0049): "re-project + drive the new". Surfaced by
 * the Play button in the run-level toolbar. Lifts a terminal (or incident-parked)
 * run back to `running` by a safe re-projection — satisfied `(node, iter)` are
 * frozen (never re-spawned, anti-#221), only the unsatisfied work runs. Distinct
 * from `retryAll` (which archives and forks a NEW run with a different id).
 */
export function reopenRun(runId: string): Promise<void> {
  return request<void>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/commands`,
    { body: { kind: "reopen_run" }, responseMode: "void", label: "reopen_run" },
  );
}

/**
 * Route a loop region by id from the Pipeline Manager (ADR-0011 / #152): end it
 * (fire its completion) so a region blocked "exhausted — unrouted" leaves the
 * region and the run proceeds. The daemon resumes the run as part of the command.
 */
export function endRegion(runId: string, regionId: string): Promise<void> {
  return request<void>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/commands`,
    { body: { kind: "end_region", region_id: regionId }, responseMode: "void", label: "end_region" },
  );
}

/**
 * Route a loop region by id from the Pipeline Manager (ADR-0011 / #152): bump it
 * (run `additionalIter` more iterations) so a region blocked "exhausted —
 * unrouted" resumes iterating. The daemon resumes the run as part of the command.
 */
export function bumpRegion(
  runId: string,
  regionId: string,
  additionalIter: number,
): Promise<void> {
  return request<void>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/commands`,
    {
      body: { kind: "bump_region", region_id: regionId, additional_iter: additionalIter },
      responseMode: "void",
      label: "bump_region",
    },
  );
}

export function retryAll(runId: string): Promise<CreateRunResponse> {
  return request<CreateRunResponse>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/commands`,
    { body: { kind: "retry_all" }, label: "retry_all" },
  );
}

export interface StartNodeResult {
  ok: boolean;
  iter?: number;
  already_running?: boolean;
}

export function startNode(
  runId: string,
  nodeId: string,
): Promise<StartNodeResult> {
  return request<StartNodeResult>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/nodes/${encodeURIComponent(nodeId)}/start`,
    { label: "start_node" },
  );
}

export function stopNode(
  runId: string,
  nodeId: string,
): Promise<void> {
  return request<void>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/nodes/${encodeURIComponent(nodeId)}/stop`,
    { responseMode: "void", label: "stop_node" },
  );
}

export interface RetryNodeResult {
  ok: boolean;
  iter: number;
  invalidated: string[];
}

export function retryNode(
  runId: string,
  nodeId: string,
): Promise<RetryNodeResult> {
  return request<RetryNodeResult>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/nodes/${encodeURIComponent(nodeId)}/retry`,
    { label: "retry_node" },
  );
}

export interface RetryPreviewResult {
  downstream: string[];
  affected_count: number;
  with_artifacts: string[];
}

export function retryNodePreview(
  runId: string,
  nodeId: string,
): Promise<RetryPreviewResult> {
  return request<RetryPreviewResult>(
    "GET",
    `/runs/${encodeURIComponent(runId)}/nodes/${encodeURIComponent(nodeId)}/retry/preview`,
    { label: "retry_preview" },
  );
}

export function cleanupRun(runId: string): Promise<void> {
  return request<void>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/commands`,
    { body: { kind: "cleanup_run" }, responseMode: "void", label: `POST /runs/${runId}/commands` },
  );
}

export function renameRun(runId: string, name: string): Promise<void> {
  return request<void>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/commands`,
    { body: { kind: "rename_run", name }, responseMode: "void", label: `POST /runs/${runId}/commands` },
  );
}

export function forgetRun(runId: string): Promise<void> {
  return request<void>(
    "DELETE",
    `/runs/${encodeURIComponent(runId)}`,
    { responseMode: "void", label: `DELETE /runs/${runId}` },
  );
}

// --- Run-scoped pipeline ---

// #550/ADR-0046: the daemon returns each node's per-harness `harnesses` map; the
// editor's pickers edit a flat `model`/`effort` view of the RESOLVED harness. Fold
// on the way in so the existing UI + library sync keep working; `serializePipeline`
// folds back on save. Applied at every pipeline-load boundary.
function foldPipelineDetail(detail: PipelineDetail): PipelineDetail {
  if (!detail?.pipeline?.nodes) return detail;
  return {
    ...detail,
    pipeline: {
      ...detail.pipeline,
      nodes: detail.pipeline.nodes.map(foldHarnessOntoNode),
    },
  };
}

export function fetchRunPipeline(runId: string): Promise<PipelineDetail> {
  return request<PipelineDetail>(
    "GET",
    `/runs/${encodeURIComponent(runId)}/pipeline`,
    { label: `GET /runs/${runId}/pipeline` },
  ).then(foldPipelineDetail);
}

export function saveRunPipeline(
  runId: string,
  yaml: string,
  prompts: Record<string, string>,
): Promise<void> {
  return request<void>(
    "PUT",
    `/runs/${encodeURIComponent(runId)}/pipeline`,
    { body: { yaml, prompts }, responseMode: "void", label: `PUT /runs/${runId}/pipeline` },
  );
}

// --- Pipeline CRUD ---

// Pin an operation to a single store. Without it the daemon resolves a bare id
// repo-then-user, so a `library` (or `user`) entry colliding with a same-named
// repo pipeline routes to the wrong file (#216). `repo`/`user`/`run` map to the
// historical default and are only forwarded when explicitly known.
function scopeQuery(scope?: string): string {
  return scope && scope !== "run" ? `?scope=${encodeURIComponent(scope)}` : "";
}

export function fetchPipeline(id: string, scope?: string): Promise<PipelineDetail> {
  return request<PipelineDetail>(
    "GET",
    `/pipelines/${encodeURIComponent(id)}${scopeQuery(scope)}`,
    { label: `GET /pipelines/${id}` },
  ).then(foldPipelineDetail);
}

export function savePipeline(
  id: string,
  yaml: string,
  prompts: Record<string, string>,
  scope?: string,
): Promise<void> {
  return request<void>(
    "PUT",
    `/pipelines/${encodeURIComponent(id)}${scopeQuery(scope)}`,
    { body: { yaml, prompts }, responseMode: "void", label: `PUT /pipelines/${id}` },
  );
}

export function createPipeline(
  name: string,
  scope: string,
): Promise<{ id: string; scope: string; path: string }> {
  return request<{ id: string; scope: string; path: string }>(
    "POST",
    "/pipelines",
    { body: { name, scope }, label: "POST /pipelines" },
  );
}

// --- Library API ---

export interface LibraryPort {
  name: string;
  repeated: boolean;
  side?: string;
  port_type?: PortType;
  frontmatter?: Record<string, FrontmatterFieldDecl> | null;
  when?: Record<string, unknown> | null;
}

export function libraryPortToPortDef(port: LibraryPort, defaultSide: PortSide): PortDef {
  return {
    name: port.name,
    repeated: port.repeated,
    side: (port.side as PortSide) ?? defaultSide,
    ...(port.port_type ? { port_type: port.port_type } : {}),
    ...(port.frontmatter ? { frontmatter: port.frontmatter } : {}),
    ...(port.when ? { when: port.when } : {}),
  };
}

export interface LibraryEntry {
  name: string;
  type: string;
  inputs: LibraryPort[];
  outputs: LibraryPort[];
  interactive: boolean;
  /** Per-node model override (#296/#345) — the node library is model-aware.
   * Absent/null ⇒ account default. */
  model?: string | null;
  /** Per-node effort override (#424) — the node library is effort-aware too.
   * Absent/null ⇒ account default. */
  effort?: string | null;
  max_iter?: number | null;
  branches?: number | null;
  prompt: string;
}

export function fetchLibrary(): Promise<LibraryEntry[]> {
  return request<LibraryEntry[]>("GET", "/library");
}

export interface LibrarySaveSpec {
  name: string;
  type: string;
  inputs: LibraryPort[];
  outputs: LibraryPort[];
  interactive: boolean;
  /** Per-node model override (#296/#345). Omit/undefined ⇒ account default. */
  model?: string | null;
  /** Per-node effort override (#424). Omit/undefined ⇒ account default. */
  effort?: string | null;
  prompt: string;
}

export function saveToLibrary(spec: LibrarySaveSpec): Promise<LibraryEntry> {
  return request<LibraryEntry>("POST", "/library", { body: spec, label: "POST /library" });
}

export function deleteFromLibrary(name: string): Promise<void> {
  return request<void>(
    "DELETE",
    `/library/${encodeURIComponent(name)}`,
    { responseMode: "void", label: `DELETE /library/${name}` },
  );
}

export interface InstantiateResult {
  spec: {
    name: string;
    type: string;
    inputs: LibraryPort[];
    outputs: LibraryPort[];
    interactive: boolean;
    /** Per-node model override (#296/#345). Null ⇒ account default. */
    model?: string | null;
    /** Per-node effort override (#424). Null ⇒ account default. */
    effort?: string | null;
  };
  prompt: string;
}

export function instantiateFromLibrary(name: string): Promise<InstantiateResult> {
  return request<InstantiateResult>(
    "POST",
    `/library/${encodeURIComponent(name)}/instantiate`,
    { label: `POST /library/${name}/instantiate` },
  );
}

/**
 * Parsed form of a single node's YAML (#345): the `POST /nodes/parse` 200 body.
 * `spec` is `LibraryEntry`-shaped (same as {@link InstantiateResult}) plus the
 * legacy `max_iter`/`branches`; `warnings` carries soft losses (coerced/unknown
 * fields) with the node still created. A hard failure throws (400 `{error}`).
 */
export interface ParseNodeResult {
  spec: {
    name: string;
    type: string;
    inputs: LibraryPort[];
    outputs: LibraryPort[];
    interactive: boolean;
    model?: string | null;
    effort?: string | null;
    /** #616 (correctif 7): the pinned harness and per-harness settings map, so a
     *  node exported with a harness axis round-trips through reimport without its
     *  model/effort re-homing onto `claude`. */
    pin_harness?: string | null;
    harnesses?: Record<string, { model?: string; effort?: string }> | null;
    max_iter?: number | string | null;
    branches?: number | null;
  };
  prompt: string;
  warnings: string[];
}

/**
 * Validate a single node's YAML on the daemon (#345 / ADR-0016) and get back a
 * canvas-instantiable spec. The front holds no YAML parser: it POSTs the raw
 * text (paste OR uploaded `.yaml`) and the daemon parses with the same serde
 * structs the pipeline parser uses. Mirror of {@link importWorkflow}: a 400
 * body carries a verbatim `error`; a 200 carries `{spec, prompt, warnings}`.
 */
export function parseNodeYaml(yaml: string): Promise<ParseNodeResult> {
  return request<ParseNodeResult>("POST", "/nodes/parse", { body: { yaml }, label: "POST /nodes/parse" });
}

export async function deletePipeline(id: string, scope?: string): Promise<void> {
  // Status-inspecting: a 409 (active runs) carries the reason in the body, so
  // raw mode keeps the bespoke branch (incl. the deliberately UNGUARDED 409
  // json). The old `{ conflict }` field had no reader — folded into status 409.
  const resp = await request<Response>(
    "DELETE",
    `/pipelines/${encodeURIComponent(id)}${scopeQuery(scope)}`,
    { responseMode: "raw" },
  );
  if (resp.status === 409) {
    const body = await resp.json();
    throw new ApiError(body.error ?? "Pipeline has active runs", { status: 409, body });
  }
  if (!resp.ok) throw new ApiError(`DELETE /pipelines/${id} failed: ${resp.status}`, { status: resp.status });
}

// --- Library Pipelines API ---

export type LibraryPipelineScope = "repo" | "user";

export interface LibraryPipelineEntry {
  id: string;
  name: string;
  scope: LibraryPipelineScope;
  node_count: number;
  modified: string | null;
  yaml: string;
  /// Parsed form of `yaml`, normalized by the daemon's pipeline parser.
  /// Divergence checks compare against this — never against the raw text,
  /// whose formatting (key order, parser-filled defaults, serializer drift)
  /// does not survive a round-trip.
  pipeline: PipelineDef;
  prompts: Record<string, string>;
}

export function fetchLibraryPipelines(): Promise<LibraryPipelineEntry[]> {
  return request<LibraryPipelineEntry[]>("GET", "/library/pipelines");
}

export interface SaveLibraryPipelineOptions {
  /// When set, save in-place at this id even if `name` changed. Required for
  /// rename-in-place: without it the daemon falls back to slug(name), which
  /// would orphan the previous entry.
  id?: string;
  scope?: LibraryPipelineScope;
}

export function saveLibraryPipeline(
  name: string,
  yaml: string,
  prompts: Record<string, string> = {},
  options: SaveLibraryPipelineOptions = {},
): Promise<{ id: string; scope: LibraryPipelineScope }> {
  return request<{ id: string; scope: LibraryPipelineScope }>(
    "POST",
    "/library/pipelines",
    {
      body: {
        name,
        yaml,
        prompts,
        ...(options.id ? { id: options.id } : {}),
        ...(options.scope ? { scope: options.scope } : {}),
      },
      label: "POST /library/pipelines",
    },
  );
}

/// Import a Claude Code workflow `.js` as a draft library pipeline (#155). The
/// `content` is the raw file text (read client-side via `File.text()` — the
/// daemon never reads `~/.claude/workflows` off disk). `filename` seeds the
/// fallback pipeline name. Returns the new id, scope, and any lossy-translation
/// warnings; a 400 body carries a verbatim `error` a real `.js` can trigger.
export function importWorkflow(
  filename: string,
  content: string,
): Promise<{ id: string; scope: string; warnings?: string[] }> {
  return request<{ id: string; scope: string; warnings?: string[] }>(
    "POST",
    "/library/import",
    { body: { filename, content }, label: "POST /library/import" },
  );
}

/// Duplicate a library pipeline template into an unlinked clone: fresh id, name
/// suffixed `(copy)` / `(copy N)`, no promotion metadata (#224). Returns the new
/// id, its scope, and the freshly-listed library entry (or null if the list
/// race-loses the just-created file).
export function duplicateLibraryPipeline(
  id: string,
): Promise<{ id: string; scope: LibraryPipelineScope; entry: LibraryPipelineEntry | null }> {
  return request<{ id: string; scope: LibraryPipelineScope; entry: LibraryPipelineEntry | null }>(
    "POST",
    `/library/pipelines/${encodeURIComponent(id)}/duplicate`,
  );
}

// --- Diff API ---

export function fetchRunDiff(runId: string): Promise<string> {
  return request<string>(
    "GET",
    `/runs/${encodeURIComponent(runId)}/diff`,
    { responseMode: "text", label: `GET /runs/${runId}/diff` },
  );
}

export function fetchNodeDiff(runId: string, nodeId: string): Promise<string> {
  return request<string>(
    "GET",
    `/runs/${encodeURIComponent(runId)}/nodes/${encodeURIComponent(nodeId)}/diff`,
    { responseMode: "text", label: `GET /runs/${runId}/nodes/${nodeId}/diff` },
  );
}

export function deleteLibraryPipeline(id: string): Promise<void> {
  return request<void>(
    "DELETE",
    `/library/pipelines/${encodeURIComponent(id)}`,
    { responseMode: "void", label: `DELETE /library/pipelines/${id}` },
  );
}

export interface PromoteResult {
  id: string;
  drifted: boolean;
}

export function promotePipeline(pipelineId: string): Promise<PromoteResult> {
  return request<PromoteResult>(
    "POST",
    `/pipelines/${encodeURIComponent(pipelineId)}/promote`,
    { label: `POST /pipelines/${pipelineId}/promote` },
  );
}
