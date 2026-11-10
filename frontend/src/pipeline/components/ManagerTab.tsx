/**
 * The Run's Pipeline Manager tab: start it on demand, attach to its session, stop it.
 */

import { useState } from "react";
import { Bot, CircleX, Loader, Play, Settings, Square } from "lucide-react";
import { Banner, Button, EmptyState, IconButton } from "../../shared/ui/kit";
import "./inspect.css";
import TmuxTerminal from "../../session/components/TmuxTerminal";
import { startRunManager, stopRunManager } from "../../run/api/manager";
import type { RunState } from "../../shared/generated/RunState";
import { useWhyNotOnRun } from "../../run/store/gestureRightsStore";

export function ManagerTab({
  run,
  session,
  onRefreshRun,
  onOpenSettings,
}: {
  run: RunState;
  session: string;
  onRefreshRun?: () => void;
  onOpenSettings?: () => void;
}) {
  const hasManager = run.has_manager ?? false;
  // CORE-42, CORE-205: Start and Stop greyed, with why, when the caller may not make them.
  const whyNotOnRun = useWhyNotOnRun(run.run_id);
  const startRefused = whyNotOnRun("start_manager");
  const stopRefused = whyNotOnRun("stop_manager");
  const [starting, setStarting] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [confirmingStop, setConfirmingStop] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const start = async () => {
    setError(null);
    setStarting(true);
    try {
      // Idempotent on the daemon (a double-click is a benign re-answer). The
      // "starting" state holds until the refreshed run state flips
      // `has_manager` — the terminal mounts then, not here.
      await startRunManager(run.run_id);
      onRefreshRun?.();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setStarting(false);
    }
  };

  const stop = async () => {
    setStopping(true);
    try {
      await stopRunManager(run.run_id);
      onRefreshRun?.();
      setConfirmingStop(false);
      // Done with the transient state: once the refreshed run state lands, the
      // empty state — not a stale "starting…" — must be what the tab shows.
      setStarting(false);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setStopping(false);
    }
  };

  if (hasManager) {
    return (
      <div className="pi-session" data-testid="manager-live">
        <div className="pi-session__bar">
          <Bot size={14} strokeWidth={1.5} aria-hidden className="ei-ic" />
          <span>Manager</span>
          <span className="d-mono ei-dim ei-trunc" title={session}>
            {session}
          </span>
          <span className="ei-row__end">
            {confirmingStop ? (
              <>
                <span className="ei-meta">Stop the manager?</span>
                <Button
                  size="sm"
                  tone="danger"
                  onClick={() => void stop()}
                  disabled={stopping}
                  data-testid="manager-stop-confirm"
                >
                  {stopping ? "Stopping…" : "Stop"}
                </Button>
                <Button size="sm" onClick={() => setConfirmingStop(false)} data-testid="manager-stop-cancel">
                  Cancel
                </Button>
              </>
            ) : (
              <IconButton
                icon={Square}
                size="sm"
                tone="danger"
                label="Stop manager"
                description="The session survives until cleanup otherwise"
                disabledReason={stopRefused}
                onClick={() => setConfirmingStop(true)}
                data-testid="manager-stop"
              />
            )}
          </span>
        </div>
        {error && (
          <div data-testid="manager-error">
            <Banner tone="failed" title="Manager" text={error} />
          </div>
        )}
        <TmuxTerminal session={session} expanded status={run.status} />
      </div>
    );
  }

  if (starting) {
    return (
      <div className="pi-center" data-testid="manager-starting">
        <EmptyState icon={Loader} spin>
          Starting the manager
        </EmptyState>
      </div>
    );
  }

  if (error) {
    return (
      <div className="pi-center" data-testid="manager-start-error">
        <EmptyState
          icon={CircleX}
          action={
            <Button size="sm" onClick={() => void start()} disabledReason={startRefused} data-testid="manager-retry">
              Retry
            </Button>
          }
        >
          <span role="alert" title={error}>
            Failed to start the manager: {error}
          </span>
        </EmptyState>
      </div>
    );
  }

  return (
    <div className="pi-center" data-testid="manager-empty-state">
      <EmptyState
        icon={Bot}
        action={
          <span className="ei-line">
            <Button
              tone="primary"
              icon={Play}
              onClick={() => void start()}
              title={startRefused ? undefined : "A conversational agent that drives this run: retries nodes, resolves merges, unblocks it"}
              disabledReason={startRefused}
              data-testid="manager-start"
            >
              Start manager
            </Button>
            <IconButton
              icon={Settings}
              label="Start with every run"
              description="Settings › Agents › Pipeline Manager"
              onClick={() => onOpenSettings?.()}
              data-testid="manager-enable-settings"
            />
          </span>
        }
      >
        No manager
      </EmptyState>
    </div>
  );
}
