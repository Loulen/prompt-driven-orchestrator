import { Clock } from "lucide-react";
import GuardTestResult from "../../../trigger/components/GuardTestResult";
import { CRON_PRESETS } from "../../../trigger/model/cronPresets";
import type { NewRunForm } from "../../store/newRun/useNewRunForm";

/** Trigger mode only: name, schedule (#160), guard command (#161/#350), overlap policy (#239). */
export default function TriggerFields({ form }: { form: NewRunForm }) {
  const {
    selectedPipeline, repoValid, input, triggerName, setTriggerName, cronPresetId,
    setCronPresetId, dailyHour, setDailyHour, dailyMinute, setDailyMinute, rawCron, setRawCron,
    guardCommand, setGuardCommand, guardTest, setGuardTest, guardTesting, guardTestError,
    setGuardTestError, allowOverlap, setAllowOverlap, maxConcurrent, setMaxConcurrent,
    promptOptional, resolvedCron, onTestGuard,
  } = form;
  return (
    <div className="flex flex-col gap-3 pb-1">
      <div className="flex flex-col gap-1.5">
        <label className="font-medium text-text-2" style={{ fontSize: "11.5px" }}>
          Trigger name
        </label>
        <input
          className="w-full rounded-md border border-line-strong bg-surface-2 px-2.5 py-1.5 text-text placeholder:text-text-3 focus:border-primary focus:outline-none"
          style={{ fontSize: "12px" }}
          placeholder="e.g. Nightly audit"
          value={triggerName}
          onChange={(e) => setTriggerName(e.target.value)}
          data-testid="trigger-name-input"
        />
      </div>

      <div className="flex flex-col gap-1.5">
        <label className="font-medium text-text-2" style={{ fontSize: "11.5px" }}>
          Schedule
        </label>
        <div className="flex flex-wrap gap-1">
          {CRON_PRESETS.map((p) => (
            <button
              key={p.id}
              onClick={() => setCronPresetId(p.id)}
              className={`rounded border px-2 py-1 font-medium transition-colors ${
                cronPresetId === p.id
                  ? "border-primary bg-selected text-primary"
                  : "border-line-strong bg-surface-2 text-text-3 hover:text-text"
              }`}
              style={{ fontSize: "11px" }}
              data-testid={`preset-${p.id}`}
            >
              {p.label}
            </button>
          ))}
          <button
            onClick={() => setCronPresetId("custom")}
            className={`rounded border px-2 py-1 font-medium transition-colors ${
              cronPresetId === "custom"
                ? "border-primary bg-selected text-primary"
                : "border-line-strong bg-surface-2 text-text-3 hover:text-text"
            }`}
            style={{ fontSize: "11px" }}
            data-testid="preset-custom"
          >
            Custom cron
          </button>
        </div>

        {cronPresetId === "daily" && (
          <div className="flex items-center gap-1.5" style={{ fontSize: "11px" }}>
            <Clock size={12} className="text-text-3" />
            <span className="text-text-3">at</span>
            <input
              type="number"
              min={0}
              max={23}
              value={dailyHour}
              onChange={(e) =>
                setDailyHour(Math.max(0, Math.min(23, Number(e.target.value) || 0)))
              }
              className="w-12 rounded border border-line-strong bg-surface-2 px-1 py-0.5 text-text focus:border-primary focus:outline-none"
              data-testid="daily-hour"
            />
            <span className="text-text-3">:</span>
            <input
              type="number"
              min={0}
              max={59}
              value={dailyMinute}
              onChange={(e) =>
                setDailyMinute(Math.max(0, Math.min(59, Number(e.target.value) || 0)))
              }
              className="w-12 rounded border border-line-strong bg-surface-2 px-1 py-0.5 text-text focus:border-primary focus:outline-none"
              data-testid="daily-minute"
            />
          </div>
        )}

        {cronPresetId === "custom" && (
          <input
            className="w-full rounded-md border border-line-strong bg-surface-2 px-2.5 py-1.5 font-mono text-text placeholder:text-text-3 focus:border-primary focus:outline-none"
            style={{ fontSize: "12px" }}
            placeholder="*/15 * * * *  (min hour dom month dow)"
            value={rawCron}
            onChange={(e) => setRawCron(e.target.value)}
            data-testid="raw-cron-input"
          />
        )}

        <span className="font-mono text-text-3" style={{ fontSize: "10px" }}>
          cron: {resolvedCron || "—"}
        </span>
        <span className="text-text-3" style={{ fontSize: "10px" }}>
          Triggers fire only while the daemon is running (best-effort in v1).
        </span>
      </div>

      {/* Guard command (Trigger mode only, #161) */}
      <div className="flex flex-col gap-1.5">
        <label className="font-medium text-text-2" style={{ fontSize: "11.5px" }}>
          Guard command (optional)
        </label>
        <textarea
          className="w-full resize-y rounded-md border border-line-strong bg-surface-2 px-2.5 py-2 font-mono text-text placeholder:text-text-3 focus:border-primary focus:outline-none"
          style={{ fontSize: "12px" }}
          rows={2}
          placeholder="e.g. gh issue list --label ready-for-agent"
          value={guardCommand}
          onChange={(e) => {
            setGuardCommand(e.target.value);
            // A verdict is stale the moment the command changes (#350).
            setGuardTest(null);
            setGuardTestError(null);
          }}
          data-testid="guard-command-input"
        />
        <span className="text-text-3" style={{ fontSize: "10.5px" }}>
          Runs before each fire from the target repo. Exit 0 fires (its stdout becomes the
          Run input); a non-zero exit skips. Bounded by a 60s timeout.
        </span>

        {/* Test guard (dry-run, #350): run the guard as typed, with no
            side effects, and show would-fire / would-skip / error. */}
        <div className="flex flex-col gap-1.5">
          <button
            type="button"
            onClick={onTestGuard}
            disabled={!repoValid || guardCommand.trim().length === 0 || guardTesting}
            title={!repoValid ? "Select a valid target repository first" : undefined}
            className="flex w-fit items-center gap-1.5 rounded-md border border-line-strong bg-surface-2 px-2.5 py-1 font-medium text-text-2 transition-colors hover:bg-hover disabled:opacity-40"
            style={{ fontSize: "11px" }}
            data-testid="guard-test-button"
          >
            {guardTesting ? "Testing…" : "Test guard"}
          </button>

          {guardTestError && (
            <div
              className="rounded-md border border-st-failed/30 bg-st-failed/14 px-2.5 py-1.5 text-st-failed"
              style={{ fontSize: "10.5px" }}
              data-testid="guard-test-error"
            >
              {guardTestError}
            </div>
          )}

          {guardTest && (
            <GuardTestResult
              result={guardTest}
              caveat={
                // Honest caveat: the guard passes, but a real fire of a
                // prompt-required pipeline with no resolved input would be
                // rejected. Same rule the server enforces, read off the
                // actual stdout — not the empty-field reject variable.
                guardTest.outcome === "pass" &&
                guardTest.stdout.trim() === "" &&
                selectedPipeline &&
                !promptOptional &&
                input.trim() === "" ? (
                  <span
                    className="text-st-blocked"
                    style={{ fontSize: "10.5px" }}
                    data-testid="guard-test-caveat"
                  >
                    Guard passes, but the resolved input would be empty — a prompt-required
                    pipeline would reject this fire.
                  </span>
                ) : undefined
              }
            />
          )}
        </div>
      </div>

      {/* Overlap policy (#239): allow concurrent fires, optionally capped. */}
      <div className="flex flex-col gap-1.5">
        <label
          className="flex items-center gap-1.5 font-medium text-text-2"
          style={{ fontSize: "11.5px" }}
        >
          <input
            type="checkbox"
            checked={allowOverlap}
            onChange={(e) => setAllowOverlap(e.target.checked)}
            className="accent-primary"
            data-testid="overlap-allow-checkbox"
          />
          Allow concurrent fires
        </label>
        {allowOverlap && (
          <div className="flex items-center gap-1.5" style={{ fontSize: "11px" }}>
            <span className="text-text-3">Max concurrent runs</span>
            <input
              type="number"
              min={1}
              placeholder="∞"
              value={maxConcurrent}
              onChange={(e) => setMaxConcurrent(e.target.value)}
              className="w-16 rounded border border-line-strong bg-surface-2 px-1.5 py-0.5 text-text focus:border-primary focus:outline-none"
              data-testid="max-concurrent-input"
            />
            <span className="text-text-3">Blank = unlimited</span>
          </div>
        )}
        <span className="text-text-3" style={{ fontSize: "10.5px" }}>
          By default a trigger skips a tick while its previous run is still live. Allow
          concurrent fires to let runs stack, optionally capped at N simultaneous runs.
        </span>
      </div>
    </div>
  );
}
