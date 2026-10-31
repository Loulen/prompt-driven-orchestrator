import { useEffect, useMemo, useState } from "react";
import { Pause, Pencil, Play } from "lucide-react";
import type { Trigger } from "../../shared/generated/Trigger";
import { fetchSettings } from "../../settings/api/settings";
import { humanizeCron } from "../model/cronPresets";
import { formatFireTs, lastOutcomeTooltip, outcomeTone, overlapLabel } from "../model/outcome";
import { tildify } from "../../lib/homePath";
import FireHistory from "./triggerDetail/FireHistory";
import GuardSection from "./triggerDetail/GuardSection";
import ExtensionDetailTab from "../../shared/extensions/ExtensionDetailTab";
import { useDetailTabs } from "../../shared/extensions/catalogue";
import ActorLabel from "../../shared/actors/ActorLabel";
import { Button, IconButton, KeyValue, Section, type KeyValueRow } from "../../shared/ui/kit";
import { ToneIcon } from "../../shared/ui/kit/tone";
import MidPath from "../../shared/ui/MidPath";
import SuspendedPill from "./triggersList/SuspendedPill";
import { suspensionTooltip } from "../model/suspension";

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
  /** « Run now » (#341) and « Edit » from the header; absent, no button. */
  onRunNow?: () => void;
  onEdit?: () => void;
  /** The name of the Trigger's Projet, when it has one. */
  projectName?: string | null;
}

/**
 * Right-panel detail view for a selected Trigger (#162): its last outcome and
 * name in the header with Run now and Edit, then folding sections — its
 * configuration (read-only, editing goes through the New Trigger dialog), its
 * guard and its dry-run, its input template, and the fire history as a table:
 * the answer to "why didn't it fire last night?".
 */
export default function TriggerDetailPanel({
  trigger,
  onSelectRun,
  refreshKey,
  promptRequired,
  onRunNow,
  onEdit,
  projectName,
}: Props) {
  // The daemon's home, to read the repo as `~/code/…` (the title keeps the full path).
  const [home, setHome] = useState<string | null>(null);
  // ADR-0081 (#31): the `trigger/detail` tabs extensions add, after Details.
  // Without one, no tab bar at all: the panel stays as it was.
  const entity = useMemo(
    () => ({
      kind: "trigger",
      id: trigger.id,
      name: trigger.name,
      attributes: trigger.attributes ?? {},
    }),
    [trigger.id, trigger.name, trigger.attributes],
  );
  const { tabs, context } = useDetailTabs(entity, "trigger/detail");
  const [tabKey, setTabKey] = useState<string | null>(null);
  const activeTab = tabs.find((t) => t.key === tabKey) ?? null;

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

  const rows: KeyValueRow[] = [
    {
      k: "Schedule",
      v: <span data-testid="trigger-detail-schedule">{humanizeCron(trigger.cron)}</span>,
      tip: humanizeCron(trigger.cron).startsWith(trigger.cron.trim())
        ? humanizeCron(trigger.cron)
        : `${humanizeCron(trigger.cron)} · ${trigger.cron}`,
    },
    {
      k: "Next",
      v: <span data-testid="trigger-detail-next">{formatFireTs(trigger.next_fire_at) ?? (trigger.enabled ? "–" : "off")}</span>,
      tip: formatFireTs(trigger.next_fire_at, "local") ?? undefined,
      mono: true,
    },
    ...(trigger.last_editor
      ? [
          {
            k: "Last editor",
            v: (
              <span data-testid="trigger-last-editor">
                <ActorLabel actor={trigger.last_editor} />
              </span>
            ),
          },
        ]
      : []),
    { k: "Pipeline", v: trigger.pipeline_name || trigger.pipeline_id, mono: true },
    ...(trigger.project_id ? [{ k: "Project", v: projectName ?? trigger.project_id }] : []),
    ...(trigger.target_repo
      ? [
          {
            k: "Repo",
            v: (
              <span className="tg-kvpath" title={trigger.target_repo} data-testid="trigger-detail-repo">
                <MidPath path={tildify(trigger.target_repo, home)} title={false} />
              </span>
            ),
            tip: trigger.target_repo,
          },
        ]
      : []),
    ...(trigger.source_branch ? [{ k: "Branch", v: trigger.source_branch, mono: true }] : []),
    { k: "Overlap", v: <span data-testid="trigger-detail-overlap">{overlapLabel(trigger)}</span>, tip: overlapLabel(trigger) },
    trigger.sandbox
      ? { k: "Sandbox", v: trigger.sandbox, mono: true }
      : { k: "Sandbox", v: <span className="ln-faint">Instance default</span>, tip: "Instance default" },
  ];

  return (
    <aside className="tg-detail" data-testid="trigger-detail-panel">
      <header className="tg-detail__head">
        <span className="tg-detail__status" role="img" aria-label={lastOutcomeTooltip(trigger)} title={lastOutcomeTooltip(trigger)}>
          <ToneIcon tone={outcomeTone(trigger.last_outcome) ?? "neutral"} size={14} />
        </span>
        <h2 className="tg-detail__name" title={trigger.name}>
          {trigger.name}
        </h2>
        {!trigger.enabled && (
          <span className="d-chip" data-testid="trigger-detail-disabled">
            disabled
          </span>
        )}
        {trigger.suspension && <SuspendedPill suspension={trigger.suspension} />}
        <span className="d-grow" />
        {/* A suspended Trigger fires neither on its schedule nor by Run now. */}
        {onRunNow && !trigger.suspension && (
          <Button size="sm" icon={Play} onClick={onRunNow} title="Run now" data-testid="trigger-detail-run-now">
            Run
          </Button>
        )}
        {onEdit && <IconButton icon={Pencil} label="Edit trigger" onClick={onEdit} data-testid="trigger-detail-edit" />}
      </header>
      {/* API 0.8: an extension suspends it — it fires neither on its schedule
          nor by Run now, until the extension lifts the suspension. */}
      {trigger.suspension && (
        <div className="tg-suspended" title={suspensionTooltip(trigger.suspension)} data-testid="trigger-detail-suspension">
          <Pause size={12} strokeWidth={1.5} aria-hidden />
          <span className="tg-suspended__word">Suspended · does not fire</span>
        </div>
      )}

      {tabs.length > 0 && (
        <div className="d-tabs tg-detail__tabs" role="tablist" aria-label="Trigger">
          {[{ key: null, label: "Details" }, ...tabs.map((t) => ({ key: t.key, label: t.contribution.label }))].map(
            ({ key, label }) => {
              const on = (activeTab?.key ?? null) === key;
              return (
                <button
                  key={key ?? "details"}
                  type="button"
                  role="tab"
                  aria-selected={on}
                  className="d-tab"
                  data-testid={`trigger-tab-${key ?? "details"}`}
                  onClick={() => setTabKey(key)}
                >
                  <span>{label}</span>
                </button>
              );
            },
          )}
        </div>
      )}

      <div className="tg-detail__body">
        {activeTab ? (
          <ExtensionDetailTab key={`${activeTab.key}:${trigger.id}`} tab={activeTab} context={context} />
        ) : (
          <>
            <Section title="Configuration">
              <KeyValue rows={rows} />
            </Section>

            {trigger.guard_command && (
              <GuardSection trigger={trigger} guardCommand={trigger.guard_command} promptRequired={promptRequired} />
            )}

            {trigger.input_template && (
              <Section title="Input template">
                <pre className="tg-pre tg-pre--plain" data-testid="trigger-detail-input">
                  {trigger.input_template}
                </pre>
              </Section>
            )}

            <FireHistory triggerId={trigger.id} refreshKey={refreshKey} onSelectRun={onSelectRun} />
          </>
        )}
      </div>
    </aside>
  );
}
