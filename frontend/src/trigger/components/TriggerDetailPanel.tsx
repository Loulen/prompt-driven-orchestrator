import { useEffect, useState } from "react";
import { Clock, FolderGit2, GitBranch, Layers, Zap } from "lucide-react";
import type { Trigger } from "../../shared/generated/Trigger";
import { fetchSettings } from "../../settings/api/settings";
import { humanizeCron } from "../model/cronPresets";
import { formatFireTs, overlapLabel } from "../model/outcome";
import { tildify } from "../../lib/homePath";
import ConfigRow from "./triggerDetail/ConfigRow";
import FireHistory from "./triggerDetail/FireHistory";
import GuardSection from "./triggerDetail/GuardSection";

interface Props {
  trigger: Trigger;
  /** Jump to the Run a "fired" history entry created. */
  onSelectRun: (runId: string) => void;
  /**
   * Bumped by the parent on every `trigger_fired` WS message (#341) so the
   * fire history refetches while the panel stays open — a manual "Run now"
   * (or a cron fire landing mid-view) appears without reselecting.
   */
  refreshKey?: number;
  /**
   * Whether the trigger's pipeline requires a prompt (#351). Drives the honest
   * "resolved input would be empty" caveat on a passing guard dry-run. Derived
   * by the parent (App.tsx) from the merged pipelines list; optional so callers
   * that omit it (and tests) still typecheck — absent ⇒ no caveat.
   */
  promptRequired?: boolean;
}

/**
 * Right-panel detail view for a selected Trigger (#162). Shows the full config
 * (read-only — editing goes through the New Run modal) plus a reverse-chrono
 * fire history: the answer to "why didn't it fire last night?". Each "fired"
 * entry links to the Run it created.
 */
export default function TriggerDetailPanel({
  trigger,
  onSelectRun,
  refreshKey,
  promptRequired,
}: Props) {
  // The daemon's home, to read the repo as `~/code/…` (the title keeps the full path).
  const [home, setHome] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    fetchSettings()
      .then((settings) => {
        if (!cancelled) setHome(settings.home ?? null);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <aside className="flex h-full flex-col overflow-y-auto bg-bg-2" data-testid="trigger-detail-panel">
      {/* Header */}
      <div className="flex items-center gap-2 border-b border-line px-3 py-2">
        <Zap size={14} className="shrink-0 text-acc" />
        <div className="min-w-0">
          <div className="truncate font-medium text-fg" style={{ fontSize: "12.5px" }}>
            {trigger.name}
          </div>
          <div className="mt-0.5 flex items-center gap-1.5 text-fg-4" style={{ fontSize: "10px" }}>
            <span>trigger</span>
            {!trigger.enabled && (
              <span
                className="rounded border border-line-strong px-1 py-px"
                style={{ fontSize: "9px" }}
                data-testid="trigger-detail-disabled"
              >
                disabled
              </span>
            )}
          </div>
        </div>
      </div>

      {/* Configuration */}
      <div className="flex flex-col gap-2 p-3" style={{ fontSize: "11.5px" }}>
        <span
          className="font-medium uppercase tracking-wider text-fg-4"
          style={{ fontSize: "10px" }}
        >
          Configuration
        </span>

        <ConfigRow icon={<Layers size={12} />} label="Pipeline">
          <span className="font-mono text-fg-2">{trigger.pipeline_name || trigger.pipeline_id}</span>
        </ConfigRow>

        {trigger.target_repo && (
          <ConfigRow icon={<FolderGit2 size={12} />} label="Repo">
            <span
              className="truncate font-mono text-fg-2"
              title={trigger.target_repo}
              data-testid="trigger-detail-repo"
            >
              {tildify(trigger.target_repo, home)}
            </span>
          </ConfigRow>
        )}

        {trigger.source_branch && (
          <ConfigRow icon={<GitBranch size={12} />} label="Branch">
            <span className="font-mono text-fg-2">{trigger.source_branch}</span>
          </ConfigRow>
        )}

        <ConfigRow icon={<Clock size={12} />} label="Schedule">
          <span className="text-fg-2" data-testid="trigger-detail-schedule">
            {humanizeCron(trigger.cron)}
          </span>
        </ConfigRow>

        <ConfigRow icon={<Layers size={12} />} label="Overlap">
          <span className="text-fg-2" data-testid="trigger-detail-overlap">
            {overlapLabel(trigger)}
          </span>
        </ConfigRow>

        {trigger.guard_command && (
          <GuardSection
            trigger={trigger}
            guardCommand={trigger.guard_command}
            promptRequired={promptRequired}
          />
        )}

        {trigger.input_template && (
          <div className="flex flex-col gap-1">
            <div className="text-fg-3">Input template</div>
            <pre
              className="whitespace-pre-wrap rounded border border-line bg-bg-3 px-2 py-1.5 font-mono text-fg-2"
              style={{ fontSize: "10.5px" }}
              data-testid="trigger-detail-input"
            >
              {trigger.input_template}
            </pre>
          </div>
        )}

        <ConfigRow icon={<Clock size={12} />} label="Next fire">
          <span className="font-mono text-fg-2">{formatFireTs(trigger.next_fire_at) ?? "—"}</span>
        </ConfigRow>
      </div>

      <FireHistory triggerId={trigger.id} refreshKey={refreshKey} onSelectRun={onSelectRun} />
    </aside>
  );
}
