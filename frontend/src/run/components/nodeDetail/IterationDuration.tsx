import { Tooltip } from "../../../shared/ui/tooltip";
import { formatDuration } from "../../model/runDuration";
import { formatTime } from "../../model/nodeDetailFormat";
import { useIterationDurations } from "../../model/iterationDuration";
import type { IterationDurations, IterationTiming } from "../../model/iterationDuration";
import type { NodeDurationMode } from "../../../lib/uiPrefs";

const DURATION_MODE_LABEL: Record<keyof IterationDurations, string> = {
  total: "Total",
  active: "Active",
  waiting: "Waiting",
};

function otherDurationMode(mode: NodeDurationMode): NodeDurationMode {
  return mode === "total" ? "active" : "total";
}

/**
 * The duration tooltip: the iteration's local times (`running` standing in for
 * the end of an iteration at work), then its Total · Active · Waiting breakdown with the
 * mode on screen marked, then the click hint.
 */
function IterationDurationTooltip({
  timing,
  durations,
  mode,
}: {
  timing: IterationTiming;
  durations: IterationDurations;
  mode: NodeDurationMode;
}) {
  // An iteration at work is `running`; one parked on a wait has no end yet.
  const ended = timing.completed_at
    ? formatTime(timing.completed_at)
    : timing.status === "running"
      ? "running"
      : "—";
  const waitOpen = !!timing.open_wait_since && timing.status === "awaiting_user";
  return (
    <div className="whitespace-normal font-mono text-2xs">
      <div className="grid grid-cols-[auto_auto] gap-x-6" data-testid="iteration-times">
        <span className="text-text-3">Started</span>
        <span className="text-right">
          {timing.started_at ? formatTime(timing.started_at) : "—"}
        </span>
        <span className="text-text-3">Ended</span>
        <span className="text-right">{ended}</span>
      </div>
      <div
        className="mt-1 grid grid-cols-[auto_auto] gap-x-6 border-t border-line pt-1"
        data-testid="iteration-breakdown"
      >
        {(["total", "active", "waiting"] as const).map((key) => {
          const current = key === mode;
          return (
            <div
              key={key}
              className={`col-span-2 grid grid-cols-subgrid ${current ? "text-text" : "text-text-3"}`}
              data-testid={`iteration-breakdown-${key}`}
              data-current={current || undefined}
            >
              <span>
                <span aria-hidden className="inline-block w-2.5">
                  {current ? "›" : ""}
                </span>
                {DURATION_MODE_LABEL[key]}
              </span>
              <span className="text-right">{formatDuration(durations[key])}</span>
            </div>
          );
        })}
      </div>
      {waitOpen && (
        <div className="mt-1 text-text-3" data-testid="iteration-active-paused">
          Active paused: waiting for you
        </div>
      )}
      <div className="mt-1 text-text-3">Click to show {otherDurationMode(mode)} time</div>
    </div>
  );
}

/**
 * The meta line's duration: `· ● 12m 4s total`. A `<button>` so the tooltip
 * opens on keyboard focus; the dotted unit reads as a defined term, the same
 * grey in both modes. A click (or Enter / Space) switches the Mode de durée.
 * Renders nothing, separator included, when there is no duration to show.
 */
export function IterationDurationLabel({
  timing,
  mode,
  onToggle,
}: {
  timing: IterationTiming;
  mode: NodeDurationMode;
  onToggle: () => void;
}) {
  const durations = useIterationDurations(timing);
  if (durations == null) return null;
  const duration = formatDuration(durations[mode]);
  const tone =
    timing.status === "running" ? "running" : timing.status === "awaiting_user" ? "awaiting" : null;
  return (
    <Tooltip
      content={<IterationDurationTooltip timing={timing} durations={durations} mode={mode} />}
    >
      <button
        type="button"
        onClick={onToggle}
        aria-label={`${duration} ${mode}. Click to show ${otherDurationMode(mode)} time`}
        data-testid="iteration-duration"
        data-duration={mode}
        className="ri-dur"
      >
        {tone && (
          <span aria-hidden data-testid="iteration-duration-dot" data-st={tone} className="ri-dur__dot" />
        )}
        <span>{duration}</span>{" "}
        <span className="ri-dur__unit" data-testid="iteration-duration-unit">
          {mode}
        </span>
      </button>
    </Tooltip>
  );
}

/** An iteration's duration in the IterSelector menu, in the mode on screen; ticks while it runs. */
export function IterOptionDuration({
  timing,
  mode,
}: {
  timing: IterationTiming;
  mode: NodeDurationMode;
}) {
  const durations = useIterationDurations(timing);
  return durations == null ? null : (
    <>
      {formatDuration(durations[mode])} {mode}
    </>
  );
}
