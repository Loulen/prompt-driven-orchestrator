import { FlaskConical } from "lucide-react";
import type { Trigger } from "../../../shared/generated/Trigger";
import { Button, Section } from "../../../shared/ui/kit";
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
  testRefused,
}: {
  trigger: Trigger;
  guardCommand: string;
  promptRequired?: boolean;
  /** CORE-205: why the caller may not dry-run a guard; Test stays, greyed. */
  testRefused?: string;
}) {
  const dryRun = useGuardDryRun(
    trigger.guard_command,
    trigger.target_repo ?? undefined,
    trigger.project_id ?? undefined,
  );
  const result = dryRun.result;

  return (
    <Section title="Guard">
      <div className="tg-guard">
        <div className="tg-guard__line">
          <code className="tg-cmd" title={guardCommand} data-testid="trigger-detail-guard">
            <span className="tg-faint" aria-hidden>
              ${" "}
            </span>
            {guardCommand}
          </code>
          <Button
            size="sm"
            icon={FlaskConical}
            onClick={dryRun.run}
            disabled={dryRun.testing}
            title={testRefused ? undefined : "Test guard (no side effect)"}
            disabledReason={testRefused}
            data-testid="guard-test-button"
          >
            {dryRun.testing ? "Testing…" : "Test"}
          </Button>
        </div>

        {dryRun.error && (
          <span className="ln-err" role="alert" data-testid="guard-test-error">
            {dryRun.error}
          </span>
        )}

        {result && (
          <GuardTestResult
            result={result}
            caveat={
              result.outcome === "pass" &&
              result.stdout.trim() === "" &&
              trigger.input_template.trim() === "" &&
              promptRequired ? (
                <span className="ln-err" data-testid="guard-test-caveat">
                  Empty input: the fire would be rejected
                </span>
              ) : undefined
            }
          />
        )}
      </div>
    </Section>
  );
}
