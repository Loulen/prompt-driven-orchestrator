import type { ReactNode } from "react";
import type { TestGuardResponse } from "../api/guard";
import { ToneIcon } from "../../shared/ui/kit/tone";
import GuardOutput from "./GuardOutput";

/** Guard dry-run verdict → word + status (#350/#351): pass → would fire,
 * skip → would skip, error → guard error. */
const GUARD_VERDICT: Record<TestGuardResponse["outcome"], { label: string; st: "done" | "skipped" | "failed" }> = {
  pass: { label: "Would fire", st: "done" },
  skip: { label: "Would skip", st: "skipped" },
  error: { label: "Guard error", st: "failed" },
};

interface GuardTestResultProps {
  result: TestGuardResponse;
  /** Optional call-site caveat, slotted between the verdict and the streams
   *  (e.g. the "resolved input would be empty" warning). */
  caveat?: ReactNode;
  /** Outer container testid; defaults to "guard-test-result". */
  "data-testid"?: string;
}

/** Shared verdict of a guard dry-run: a line edged in the status colour (icon,
 * word, exit code), an optional caveat, then the captured streams. Purely
 * presentational. Used by the New-trigger dry-run (#350) and the detail-panel
 * dry-run (#351). */
export default function GuardTestResult({
  result,
  caveat,
  "data-testid": testId = "guard-test-result",
}: GuardTestResultProps) {
  const v = GUARD_VERDICT[result.outcome];
  return (
    <div className="tg-verdict" data-st={v.st} data-testid={testId}>
      <span className="tg-verdict__head">
        <ToneIcon tone={v.st} />
        <span data-testid="guard-test-verdict">{v.label}</span>
        {result.exit_code != null && <span className="tg-faint tg-mono">exit {result.exit_code}</span>}
      </span>
      {caveat}
      <GuardOutput stdout={result.stdout} stderr={result.stderr} data-testid="guard-test-output" />
    </div>
  );
}
