import type { Trigger } from "../../shared/generated/Trigger";
import type { TriggerListEntry } from "../../shared/generated/TriggerListEntry";
import type { TriggerFire } from "../../shared/generated/TriggerFire";
import type { AgentChoice } from "../../shared/generated/AgentChoice";
import type { SkillRef } from "../../shared/generated/SkillRef";
import { ApiError, request } from "../../shared/http/client";
import type { TargetRepoInput } from "../../run/api/createRun";

export interface CreateTriggerRequest {
  name: string;
  pipeline_id: string;
  cron: string;
  input_template?: string;
  target_repo?: string;
  /** ADR-0087: the Projet its Runs are launched in; omitted for none. */
  project_id?: string;
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
  agent_choice?: AgentChoice | null;
  /** #669: the Run-tier skills every fired Run carries. Omit for none. */
  skills?: SkillRef[];
  /** Whether Runs this Trigger fires are auto-named (#338). Seeded from the instance
   *  default in the modal; omit → the server defaults to `true` (pre-#338 behaviour). */
  auto_name?: boolean;
}

export function fetchTriggers(): Promise<TriggerListEntry[]> {
  return request<TriggerListEntry[]>("GET", "/triggers");
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
  /** ADR-0087: the Projet its Runs are launched in; `null` for none. */
  project_id?: string | null;
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
  agent_choice?: AgentChoice | null;
  /** #669: replace the Run-tier skills wholesale; `[]` clears. */
  skills?: SkillRef[];
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
