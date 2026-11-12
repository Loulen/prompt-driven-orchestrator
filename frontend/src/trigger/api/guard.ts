import { request } from "../../shared/http/client";

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
 * `next_fire_at` bump) and returns the verdict. `target_repo` is the repository
 * the guard runs in: like a real fire, the daemon refuses (400) a guard nobody
 * named a repository for. `project_id` is the Project the Trigger is (or will
 * be) in: the guard runs with what the extensions give the caller there, as a
 * fire runs it with what they give the Trigger's last editor.
 */
export function testGuard(
  guard_command: string,
  target_repo?: string,
  project_id?: string,
): Promise<TestGuardResponse> {
  return request<TestGuardResponse>(
    "POST",
    "/triggers/guard/test",
    {
      body: { guard_command, target_repo, project_id },
      label: "POST /triggers/guard/test",
    },
  );
}
