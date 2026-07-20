import type { ReactNode } from "react";
import type { TestGuardResponse } from "../api";
import GuardOutput from "./GuardOutput";

/** Guard dry-run verdict → label + status color (#350/#351). 1:1 with the three
 * `GuardResult` variants: pass → would fire (green), skip → would skip (amber),
 * error → guard error (red). */
const GUARD_VERDICT: Record<TestGuardResponse["outcome"], { label: string; cls: string }> = {
  pass: { label: "Would fire", cls: "border-st-done/30 bg-st-done-bg text-st-done" },
  skip: { label: "Would skip", cls: "border-st-paused/30 bg-st-paused-bg text-st-paused" },
  error: { label: "Guard error", cls: "border-st-failed/30 bg-st-failed-bg text-st-failed" },
};

interface GuardTestResultProps {
  result: TestGuardResponse;
  /** Optional call-site caveat, slotted between the verdict and the streams
   *  (e.g. the "resolved input would be empty" warning). */
  caveat?: ReactNode;
  /** Outer container testid; defaults to "guard-test-result". */
  "data-testid"?: string;
}

/** Shared verdict card for a guard dry-run: colored badge + optional caveat +
 * the captured GuardOutput streams. Purely presentational — no fetching. Used by
 * the New-trigger dry-run (#350) and the detail-panel dry-run (#351). */
export default function GuardTestResult({
  result,
  caveat,
  "data-testid": testId = "guard-test-result",
}: GuardTestResultProps) {
  return (
    <div
      className={`flex flex-col gap-1.5 rounded-md border px-2.5 py-2 ${GUARD_VERDICT[result.outcome].cls}`}
      data-testid={testId}
    >
      <span className="font-medium" data-testid="guard-test-verdict" style={{ fontSize: "11.5px" }}>
        {GUARD_VERDICT[result.outcome].label}
      </span>
      {caveat}
      <GuardOutput
        stdout={result.stdout}
        stderr={result.stderr}
        exitCode={result.exit_code}
        data-testid="guard-test-output"
      />
    </div>
  );
}
