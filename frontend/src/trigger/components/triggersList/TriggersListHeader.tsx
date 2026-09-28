import { AlertCircle, PauseCircle, Plus } from "lucide-react";

/**
 * The Triggers tab's header — the #348 master switch and "New Trigger" — and,
 * while paused, the amber banner. The two channels (global pause vs per-row
 * disabled) stay visually distinct: the banner never grays a row.
 */
export default function TriggersListHeader({
  paused,
  onTogglePause,
  onNewTrigger,
}: {
  paused: boolean;
  onTogglePause?: () => void;
  onNewTrigger: () => void;
}) {
  return (
    <>
      <div
        className="flex h-[36px] shrink-0 items-center border-b border-line px-3 font-medium text-fg-2"
        style={{ fontSize: "11.5px" }}
      >
        Triggers
        <div className="ml-auto flex items-center gap-2">
          {/* #348 master switch — global kill-switch, deliberately distinct from
              the per-row enable/disable toggle (amber st-await when active, vs the
              per-row green/grey Power). */}
          <span
            role="switch"
            aria-checked={paused}
            aria-label={paused ? "Resume all triggers" : "Pause all triggers"}
            title={
              paused
                ? "Resume all triggers"
                : "Pause all triggers (global kill-switch)"
            }
            onClick={onTogglePause}
            data-testid="triggers-pause-switch"
            className={`flex cursor-pointer items-center gap-1 rounded px-1.5 py-0.5 transition-colors ${
              paused
                ? "bg-st-await-bg text-st-await"
                : "text-fg-4 hover:bg-bg-4 hover:text-fg-2"
            }`}
            style={{ fontSize: "10.5px" }}
          >
            <PauseCircle size={12} />
            {paused ? "Paused" : "Pause all"}
          </span>
          <button
            onClick={onNewTrigger}
            className="flex cursor-pointer items-center gap-1 rounded bg-acc px-1.5 py-0.5 font-medium text-on-acc transition-colors hover:bg-acc-dim"
            style={{ fontSize: "10.5px" }}
          >
            <Plus size={10} />
            New Trigger
          </button>
        </div>
      </div>

      {paused && (
        <div
          className="flex items-center gap-2 border-b border-st-await/30 bg-st-await-bg px-3 py-2"
          data-testid="triggers-paused-banner"
        >
          <AlertCircle size={14} className="shrink-0 text-st-await" />
          <span
            className="text-st-await"
            style={{ fontSize: "11.5px", fontWeight: 500 }}
          >
            All triggers paused — scheduled fires are suppressed. Manual “Run now”
            still works.
          </span>
        </div>
      )}
    </>
  );
}
