import { useState } from "react";
import { ChevronDown, ChevronRight, History } from "lucide-react";
import type { TriggerFire } from "../../../shared/generated/TriggerFire";
import { formatFireTs, outcomeDot } from "../../model/outcome";
import { useTriggerFires } from "../../store/useTriggerFires";
import GuardOutput from "../GuardOutput";

/**
 * A Trigger's reverse-chrono fire history: the answer to "why didn't it fire
 * last night?". Each "fired" entry links to the Run it created.
 */
export default function FireHistory({
  triggerId,
  refreshKey,
  onSelectRun,
}: {
  triggerId: string;
  refreshKey?: number;
  onSelectRun: (runId: string) => void;
}) {
  const { fires, loading } = useTriggerFires(triggerId, refreshKey);

  return (
    <div className="flex flex-col gap-2 border-t border-line p-3" style={{ fontSize: "11.5px" }}>
      <div className="flex items-center gap-1.5">
        <History size={12} className="text-fg-3" />
        <span
          className="font-medium uppercase tracking-wider text-fg-4"
          style={{ fontSize: "10px" }}
        >
          Fire history
        </span>
      </div>

      {loading ? (
        <div className="text-fg-4" style={{ fontSize: "10.5px" }}>
          Loading…
        </div>
      ) : fires.length === 0 ? (
        <div className="text-fg-4" style={{ fontSize: "10.5px" }} data-testid="fire-history-empty">
          No fires yet — entries appear here each time the trigger evaluates.
        </div>
      ) : (
        <div className="flex flex-col gap-1.5" data-testid="fire-history">
          {fires.map((f) => (
            <FireEntry key={f.id} fire={f} onSelectRun={onSelectRun} />
          ))}
        </div>
      )}
    </div>
  );
}

function FireEntry({
  fire,
  onSelectRun,
}: {
  fire: TriggerFire;
  onSelectRun: (runId: string) => void;
}) {
  const [showOutput, setShowOutput] = useState(false);

  // The disclosure is scoped strictly to `guard-exit-nonzero` rows (#244/D2):
  // `guard-error` already surfaces its detail via `reason`, and legacy/other
  // rows carry no captured streams. `!= null` on the code so a legit `0` counts.
  const hasGuardOutput =
    fire.outcome === "guard-exit-nonzero" &&
    (fire.guard_exit_code != null ||
      !!fire.guard_stdout?.trim() ||
      !!fire.guard_stderr?.trim());

  return (
    <div
      className="flex flex-col gap-0.5 rounded border border-line bg-bg-3 px-2 py-1.5"
      data-testid="fire-entry"
    >
      <div className="flex items-center gap-1.5">
        <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${outcomeDot(fire.outcome)}`} />
        <span className="font-medium text-fg-2">{fire.outcome}</span>
        {fire.source === "manual" && (
          <span
            className="rounded border border-line-strong px-1 py-px text-fg-3"
            style={{ fontSize: "9px" }}
            data-testid="fire-source-manual"
          >
            manual
          </span>
        )}
        <span className="ml-auto font-mono text-fg-4" style={{ fontSize: "10px" }}>
          {formatFireTs(fire.ts)}
        </span>
      </div>
      {fire.reason && (
        <div className="text-fg-3" style={{ fontSize: "10.5px" }}>
          {fire.reason}
        </div>
      )}
      {fire.run_id && (
        <button
          onClick={() => onSelectRun(fire.run_id!)}
          className="self-start font-mono text-acc hover:underline"
          style={{ fontSize: "10.5px" }}
          data-testid="fire-run-link"
          title="Open the run this fire created"
        >
          {fire.run_id}
        </button>
      )}

      {hasGuardOutput && (
        <>
          <button
            type="button"
            onClick={() => setShowOutput((v) => !v)}
            className="flex cursor-pointer items-center gap-1 self-start text-fg-3 transition-colors hover:text-fg-2"
            style={{ fontSize: "10.5px" }}
            data-testid="fire-guard-output-toggle"
          >
            {showOutput ? <ChevronDown size={11} /> : <ChevronRight size={11} />}
            Guard output
          </button>
          {showOutput && (
            <GuardOutput
              stdout={fire.guard_stdout}
              stderr={fire.guard_stderr}
              exitCode={fire.guard_exit_code}
              data-testid="fire-guard-output"
            />
          )}
        </>
      )}
    </div>
  );
}
