import { Fragment, useState } from "react";
import { ChevronDown, ChevronRight, Clock, Hand } from "lucide-react";
import type { TriggerFire } from "../../../shared/generated/TriggerFire";
import { Section } from "../../../shared/ui/kit";
import { ToneIcon } from "../../../shared/ui/kit/tone";
import { formatFireTs, outcomeTone, outcomeWord } from "../../model/outcome";
import { useTriggerFires } from "../../store/useTriggerFires";
import GuardOutput from "../GuardOutput";

/**
 * A Trigger's reverse-chrono fire history, a table: the answer to "why didn't
 * it fire last night?". Each row: the outcome's icon, when, how (the schedule
 * or Run now), the outcome and its reason, the Run it created, the guard's
 * exit code; a guard that skipped unfolds its output.
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
    <Section title="Fires" count={loading ? undefined : fires.length} flush>
      {loading ? (
        <div className="d-empty--row tg-empty">Loading…</div>
      ) : fires.length === 0 ? (
        <div className="d-empty--row tg-empty" data-testid="fire-history-empty">
          No fire
        </div>
      ) : (
        <table className="tg-fires" data-testid="fire-history">
          <thead>
            <tr>
              <th aria-label="Outcome icon" />
              <th>Time</th>
              <th aria-label="Source" />
              <th>Outcome</th>
              <th>Run</th>
              <th className="tg-r">Exit</th>
            </tr>
          </thead>
          <tbody>
            {fires.map((f) => (
              <FireEntry key={f.id} fire={f} onSelectRun={onSelectRun} />
            ))}
          </tbody>
        </table>
      )}
    </Section>
  );
}

function FireEntry({ fire, onSelectRun }: { fire: TriggerFire; onSelectRun: (runId: string) => void }) {
  const [showOutput, setShowOutput] = useState(false);

  // The disclosure is scoped strictly to `guard-exit-nonzero` rows (#244/D2):
  // `guard-error` already surfaces its detail via `reason`, and legacy/other
  // rows carry no captured streams. The exit code has its own column.
  const hasGuardOutput =
    fire.outcome === "guard-exit-nonzero" && (!!fire.guard_stdout?.trim() || !!fire.guard_stderr?.trim());
  const word = outcomeWord(fire.outcome);
  const manual = fire.source === "manual";
  const SourceIcon = manual ? Hand : Clock;

  return (
    <Fragment>
      <tr className="tg-fire" data-testid="fire-entry" data-outcome={fire.outcome}>
        <td className="tg-fire__ic">
          <ToneIcon tone={outcomeTone(fire.outcome) ?? "skipped"} />
        </td>
        <td className="tg-mono tg-fire__ts" title={fire.ts}>
          {formatFireTs(fire.ts)}
        </td>
        <td className="tg-fire__ic">
          <span
            role="img"
            aria-label={manual ? "Run now" : "Schedule"}
            title={manual ? "Run now" : "Schedule"}
            data-testid={manual ? "fire-source-manual" : "fire-source-cron"}
          >
            <SourceIcon size={12} strokeWidth={1.5} aria-hidden />
          </span>
        </td>
        <td className="tg-fire__out" title={fire.reason ? `${word} · ${fire.reason}` : `${word} (${fire.outcome})`}>
          {hasGuardOutput && (
            <button
              type="button"
              onClick={() => setShowOutput((v) => !v)}
              className="tg-fire__toggle"
              aria-expanded={showOutput}
              aria-label="Guard output"
              title="Guard output"
              data-testid="fire-guard-output-toggle"
            >
              {showOutput ? <ChevronDown size={12} strokeWidth={1.5} /> : <ChevronRight size={12} strokeWidth={1.5} />}
            </button>
          )}
          <span className="tg-fire__word">{word}</span>
          {fire.reason && <span className="tg-faint tg-fire__reason">{fire.reason}</span>}
        </td>
        <td className="tg-fire__run">
          {fire.run_id ? (
            <button
              type="button"
              onClick={() => onSelectRun(fire.run_id!)}
              className="tg-link tg-mono"
              data-testid="fire-run-link"
              title={`Open ${fire.run_id}`}
            >
              {fire.run_id}
            </button>
          ) : (
            <span className="tg-faint">–</span>
          )}
        </td>
        <td className="tg-mono tg-r">{fire.guard_exit_code ?? ""}</td>
      </tr>
      {hasGuardOutput && showOutput && (
        <tr className="tg-fire__more">
          <td />
          <td colSpan={5}>
            <GuardOutput
              stdout={fire.guard_stdout}
              stderr={fire.guard_stderr}
              data-testid="fire-guard-output"
            />
          </td>
        </tr>
      )}
    </Fragment>
  );
}
