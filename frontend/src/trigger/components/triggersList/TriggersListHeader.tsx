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
}: {
  paused: boolean;
  onTogglePause?: () => void;
  onNewTrigger: () => void;
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
          data-testid="triggers-pause-switch"
        />
        <IconButton icon={Plus} label="New trigger" onClick={onNewTrigger} />
      </LeftPaneBar>

      {paused && (
        <div data-testid="triggers-paused-banner">
          <Banner
            tone="awaiting"
            title="All triggers paused"
            text="Scheduled fires are suppressed; Run now still works."
            actions={
              onTogglePause && (
                <Button size="sm" tone="ghost" onClick={onTogglePause}>
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
