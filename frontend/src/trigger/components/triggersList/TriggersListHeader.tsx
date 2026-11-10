import { Pause, Play, Plus } from "lucide-react";
import { Banner, Button, IconButton } from "../../../shared/ui/kit";
import LeftPaneBar from "../../../shared/ui/LeftPaneBar";

/**
 * The Triggers tab's header — the #348 master switch and New trigger — and,
 * while paused, the amber banner. The two channels (global pause vs per-row
 * disabled) stay visually distinct: the banner never grays a row.
 */
export default function TriggersListHeader({
  paused,
  onTogglePause,
  onNewTrigger,
  pauseRefused,
  createRefused,
}: {
  paused: boolean;
  onTogglePause?: () => void;
  onNewTrigger: () => void;
  /** CORE-205: why the caller may not pause (nor resume) all Triggers; greyed. */
  pauseRefused?: string;
  /** CORE-205: why the caller may not create a Trigger; greyed. */
  createRefused?: string;
}) {
  return (
    <>
      <LeftPaneBar title="Triggers">
        {/* #348 master switch — global kill-switch, deliberately distinct from
            the per-row enable/disable toggle. */}
        <IconButton
          icon={paused ? Play : Pause}
          label={paused ? "Resume all triggers" : "Pause all triggers"}
          role="switch"
          aria-checked={paused}
          onClick={onTogglePause}
          disabledReason={pauseRefused}
          data-testid="triggers-pause-switch"
        />
        <IconButton
          icon={Plus}
          label="New trigger"
          onClick={onNewTrigger}
          disabledReason={createRefused}
          data-testid="triggers-new"
        />
      </LeftPaneBar>

      {paused && (
        <div data-testid="triggers-paused-banner">
          <Banner
            tone="awaiting"
            title="All triggers paused"
            text="Scheduled fires are suppressed; Run now still works."
            actions={
              onTogglePause && (
                <Button size="sm" tone="ghost" onClick={onTogglePause} disabledReason={pauseRefused}>
                  Resume
                </Button>
              )
            }
          />
        </div>
      )}
    </>
  );
}
