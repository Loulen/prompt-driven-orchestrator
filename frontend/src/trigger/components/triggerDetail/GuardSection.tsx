import { Shield } from "lucide-react";
import type { Trigger } from "../../../shared/generated/Trigger";
import { useGuardDryRun } from "../../store/useGuardDryRun";
import GuardTestResult from "../GuardTestResult";

/**
 * The saved guard of a Trigger and its dry-run (#351): run the saved guard with
 * zero side effects and show would-fire / would-skip / error. Rendered only when
 * the Trigger has a guard.
 */
export default function GuardSection({
  trigger,
  guardCommand,
  promptRequired,
}: {
  trigger: Trigger;
  guardCommand: string;
  promptRequired?: boolean;
}) {
  const dryRun = useGuardDryRun(trigger.guard_command, trigger.target_repo ?? undefined);
  const result = dryRun.result;

  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-center gap-1.5 text-fg-3">
        <Shield size={12} />
        <span>Guard</span>
      </div>
      <pre
        className="overflow-x-auto rounded border border-line bg-bg-3 px-2 py-1.5 font-mono text-fg-2"
        style={{ fontSize: "10.5px" }}
        data-testid="trigger-detail-guard"
      >
        {guardCommand}
      </pre>

      <button
        type="button"
        onClick={dryRun.run}
        disabled={dryRun.testing}
        className="flex w-fit items-center gap-1.5 rounded-md border border-line-strong bg-bg-3 px-2 py-1 font-medium text-fg-2 transition-colors hover:bg-bg-4 disabled:opacity-40"
        style={{ fontSize: "10.5px" }}
        data-testid="guard-test-button"
      >
        {dryRun.testing ? "Testing…" : "Test guard"}
      </button>

      {dryRun.error && (
        <div
          className="rounded-md border border-st-failed/30 bg-st-failed-bg px-2.5 py-1.5 text-st-failed"
          style={{ fontSize: "10px" }}
          data-testid="guard-test-error"
        >
          {dryRun.error}
        </div>
      )}

      {result && (
        <GuardTestResult
          result={result}
          caveat={
            result.outcome === "pass" &&
            result.stdout.trim() === "" &&
            trigger.input_template.trim() === "" &&
            promptRequired ? (
              <span className="text-st-blocked" style={{ fontSize: "10px" }} data-testid="guard-test-caveat">
                Guard passes, but the resolved input would be empty — a prompt-required
                pipeline would reject this fire.
              </span>
            ) : undefined
          }
        />
      )}
    </div>
  );
}
