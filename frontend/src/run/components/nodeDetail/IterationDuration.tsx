import { Tooltip, TooltipProvider } from "../../../shared/ui/tooltip";
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
    <div className="whitespace-normal font-mono" style={{ fontSize: "10px" }}>
      <div className="grid grid-cols-[auto_auto] gap-x-6" data-testid="iteration-times">
        <span className="text-fg-3">Started</span>
        <span className="text-right">
          {timing.started_at ? formatTime(timing.started_at) : "—"}
        </span>
        <span className="text-fg-3">Ended</span>
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
              className={`col-span-2 grid grid-cols-subgrid ${current ? "text-fg" : "text-fg-3"}`}
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
        <div className="mt-1 text-fg-3" data-testid="iteration-active-paused">
          Active paused: waiting for you
        </div>
      )}
      <div className="mt-1 text-fg-3">Click to show {otherDurationMode(mode)} time</div>
    </div>
  );
}

/**
 * The meta line's duration: `· ● 12m 4s total`. A `<button>` so the tooltip
 * opens on keyboard focus; the dotted unit reads as a defined term. A click (or
 * Enter / Space) switches the Mode de durée; off the default, the unit takes
 * the app green (`acc-app`), not the Inspector's amber, which would blend with
 * the awaiting dot. Renders nothing, separator included, when there is no
 * duration to show.
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
  const dot =
    timing.status === "running"
      ? "bg-st-running animate-pulse"
      : timing.status === "awaiting_user"
        ? "bg-st-await"
        : null;
  const unitColor =
    mode === "active"
      ? "text-acc-app decoration-acc-app"
      : "text-fg-4 decoration-fg-4 group-hover:decoration-fg-3";
  return (
    // A local provider: the panel also renders outside the App's (tests).
    <TooltipProvider>
      <span> · </span>
      <Tooltip
        content={<IterationDurationTooltip timing={timing} durations={durations} mode={mode} />}
      >
        <button
          type="button"
          onClick={onToggle}
          aria-label={`${duration} ${mode}. Click to show ${otherDurationMode(mode)} time`}
          data-testid="iteration-duration"
          data-mode={mode}
          className="group flex cursor-pointer items-center gap-1 rounded px-1 py-0.5 font-mono text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg-2"
          style={{ fontSize: "10px" }}
        >
          {dot && (
            <span
              aria-hidden
              data-testid="iteration-duration-dot"
              className={`h-1.5 w-1.5 shrink-0 rounded-full ${dot}`}
            />
          )}
          <span>{duration}</span>{" "}
          <span
            className={`underline decoration-dotted underline-offset-2 ${unitColor}`}
            data-testid="iteration-duration-unit"
          >
            {mode}
          </span>
        </button>
      </Tooltip>
    </TooltipProvider>
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
