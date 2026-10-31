import { FlaskConical } from "lucide-react";
import GuardTestResult from "../../../trigger/components/GuardTestResult";
import { CRON_PRESETS, CRON_TIME_ZONE, cronDependsOnZone } from "../../../trigger/model/cronPresets";
import { Button, TextInput } from "../../../shared/ui/kit";
import FormRow from "../../../shared/ui/FormRow";
import { AutoNameBox } from "./RunNameSection";
import type { NewRunForm } from "../../store/newRun/useNewRunForm";

/** The short names of the presets in the segmented schedule. */
const SHORT: Record<string, string> = { every_15_min: "15 min", hourly: "Hourly", daily: "Daily" };

const clamp = (v: string, max: number) => Math.max(0, Math.min(max, Number(v) || 0));

/** Trigger mode only, the When section: name, schedule (#160), guard (#161/#350), overlap (#239). */
export default function TriggerFields({ form }: { form: NewRunForm }) {
  const {
    selectedPipeline, repoValid, input, triggerName, setTriggerName, cronPresetId,
    setCronPresetId, dailyHour, setDailyHour, dailyMinute, setDailyMinute, rawCron, setRawCron,
    guardCommand, setGuardCommand, guardTest, setGuardTest, guardTesting, guardTestError,
    setGuardTestError, allowOverlap, setAllowOverlap, maxConcurrent, setMaxConcurrent,
    promptOptional, resolvedCron, onTestGuard,
  } = form;
  const presets = [...CRON_PRESETS.map((p) => ({ id: p.id, label: SHORT[p.id] ?? p.label, title: p.label })), { id: "custom" as const, label: "Cron", title: "Custom cron" }];
  return (
    <>
      <FormRow label="Name" htmlFor="trigger-name">
        <TextInput
          id="trigger-name"
          className="ln-grow"
          placeholder="Untitled trigger"
          value={triggerName}
          onChange={(e) => setTriggerName(e.target.value)}
          data-autofocus=""
          data-testid="trigger-name-input"
        />
        <AutoNameBox form={form} />
      </FormRow>

      <FormRow label="Schedule" title="Fires while the daemon runs">
        <div className="d-seg" role="radiogroup" aria-label="Schedule">
          {presets.map((p) => (
            <button
              key={p.id}
              type="button"
              role="radio"
              aria-checked={cronPresetId === p.id}
              title={p.title}
              className="d-seg__btn"
              onClick={() => setCronPresetId(p.id)}
              data-testid={`preset-${p.id}`}
            >
              {p.label}
            </button>
          ))}
        </div>
        {cronPresetId === "daily" && (
          <span className="ln-time">
            <TextInput
              type="number"
              min={0}
              max={23}
              className="ln-w-num"
              aria-label="Hour"
              value={dailyHour}
              onChange={(e) => setDailyHour(clamp(e.target.value, 23))}
              data-testid="daily-hour"
            />
            <span>:</span>
            <TextInput
              type="number"
              min={0}
              max={59}
              className="ln-w-num"
              aria-label="Minute"
              value={dailyMinute}
              onChange={(e) => setDailyMinute(clamp(e.target.value, 59))}
              data-testid="daily-minute"
            />
          </span>
        )}
        {cronPresetId === "custom" ? (
          <TextInput
            mono
            className="ln-grow"
            placeholder="*/15 * * * *"
            title="min hour dom month dow"
            aria-label="Cron"
            value={rawCron}
            onChange={(e) => setRawCron(e.target.value)}
            data-testid="raw-cron-input"
          />
        ) : (
          <span className="ln-mono ln-faint ln-cron" data-testid="resolved-cron">
            {resolvedCron || "–"}
          </span>
        )}
        {cronDependsOnZone(resolvedCron) && (
          <span className="ln-mono ln-faint" data-testid="cron-time-zone">
            {CRON_TIME_ZONE}
          </span>
        )}
      </FormRow>

      <FormRow label="Guard" top title="Exit 0 fires, its stdout is the input; non-zero skips. 60 s timeout.">
        <div className="ln-col ln-grow">
          <div className="ln-line">
            <TextInput
              mono
              className="ln-grow"
              placeholder="gh issue list --label ready-for-agent"
              aria-label="Guard command"
              spellCheck={false}
              value={guardCommand}
              onChange={(e) => {
                setGuardCommand(e.target.value);
                // A verdict is stale the moment the command changes (#350).
                setGuardTest(null);
                setGuardTestError(null);
              }}
              data-testid="guard-command-input"
            />
            <Button
              icon={FlaskConical}
              onClick={onTestGuard}
              disabled={!repoValid || guardCommand.trim().length === 0 || guardTesting}
              title={!repoValid ? "Choose a repository first" : "Test guard"}
              data-testid="guard-test-button"
            >
              {guardTesting ? "Testing…" : "Test"}
            </Button>
          </div>
          {guardTestError && (
            <span className="ln-err" role="alert" data-testid="guard-test-error">
              {guardTestError}
            </span>
          )}
          {guardTest && (
            <GuardTestResult
              result={guardTest}
              caveat={
                // The guard passes, but a real fire of a prompt-required
                // pipeline with no resolved input would be rejected: the
                // server's rule, read off the actual stdout.
                guardTest.outcome === "pass" &&
                guardTest.stdout.trim() === "" &&
                selectedPipeline &&
                !promptOptional &&
                input.trim() === "" ? (
                  <span className="ln-err" data-testid="guard-test-caveat">
                    Empty input: the fire would be rejected
                  </span>
                ) : undefined
              }
            />
          )}
        </div>
      </FormRow>

      <FormRow label="Overlap" title="Off: a tick is skipped while the previous run is live">
        <button
          type="button"
          role="switch"
          aria-checked={allowOverlap}
          aria-label="Allow concurrent fires"
          title="Allow concurrent fires"
          className="d-switch"
          onClick={() => setAllowOverlap(!allowOverlap)}
          data-testid="overlap-allow-checkbox"
        >
          <span className="d-switch__knob" aria-hidden />
        </button>
        <span className="ln-faint">max</span>
        <TextInput
          type="number"
          min={1}
          className="ln-w-num"
          placeholder="∞"
          aria-label="Max concurrent runs"
          title="Blank: no limit"
          value={maxConcurrent}
          disabled={!allowOverlap}
          onChange={(e) => setMaxConcurrent(e.target.value)}
          data-testid="max-concurrent-input"
        />
      </FormRow>
    </>
  );
}
