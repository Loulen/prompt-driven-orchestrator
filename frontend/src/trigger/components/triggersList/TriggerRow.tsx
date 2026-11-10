import { useMemo } from "react";
import { CalendarClock, Circle, Copy, Layers, Pencil, Play, Power, ShieldQuestion, SkipForward, Trash2 } from "lucide-react";
import ExtensionDecorations from "../../../shared/extensions/ExtensionDecorations";
import { ExtensionMenuGroups } from "../../../shared/extensions/ExtensionActions";
import type { DecorationContext, TriggerEntity } from "../../../shared/extensions/contract";
import { DropdownMenuSeparator } from "../../../shared/ui/dropdown-menu";
import RowMenu, { ROW_MENU_EXTENSION_ITEM, RowMenuItem } from "../../../shared/ui/RowMenu";
import { reasonsOf } from "../../../shared/ui/menuReasons";
import { toast } from "../../../shared/ui/toast";
import { StatusIcon, Tooltip } from "../../../shared/ui/kit";
import StatusMark from "../../../shared/ui/leftList/StatusMark";
import HoverBar, { RowAction } from "../../../shared/ui/leftList/HoverBar";
import { fmtAge, fmtUntil } from "../../../shared/ui/leftList/format";
import { useListClock } from "../../../shared/ui/leftList/clock";
import type { Trigger } from "../../../shared/generated/Trigger";
import { humanizeCron } from "../../model/cronPresets";
import { NOT_SCHEDULED_HINT, fireRefusalOf, lastOutcomeTooltip, outcomeTone, overlapLabel } from "../../model/outcome";
import SuspendedPill from "./SuspendedPill";
import { suspensionTooltip } from "../../model/suspension";
import { copyToClipboard } from "../../../shared/lib/clipboard";
import { useAppearancePreview, useElementMarks } from "../../../hooks/useElementMarks";
import { useAppearance } from "../../../hooks/useAppearance";
import { isVisible } from "../../../lib/appearance";
import { hasSecondLine, isSecondLineElement, TRIGGER_ROW } from "../../model/triggerRowElements";
import type { TriggerRefusals } from "../../store/useTriggerRefusals";

interface Props {
  trigger: Trigger;
  /** The Trigger the right panel shows. */
  isSelected: boolean;
  /** Multi-selected (#577). */
  rowSelected: boolean;
  /** Some Trigger of the list is multi-selected: every mark shows its box. */
  selecting?: boolean;
  /** #348: every scheduled fire is paused. */
  paused?: boolean;
  onSelect: () => void;
  /** The multi-select control was clicked (shift ⇒ a range). */
  onSelectControl: (shiftKey: boolean) => void;
  onRunNow?: () => void;
  onEdit?: () => void;
  onDelete: () => void;
  onToggle: () => void;
  /**
   * CORE-205: the gestures the caller may not make, and why: each stays,
   * greyed, its tooltip (or the menu's foot) saying the sentence.
   */
  refused?: TriggerRefusals;
}

/** When it fires next: in words, or why it does not. */
function nextOf(t: Trigger, paused: boolean, now: number): { text: string; title: string; dim: boolean } {
  const schedule = humanizeCron(t.cron);
  if (t.suspension) return { text: "—", title: `Suspended: ${suspensionTooltip(t.suspension)}`, dim: true };
  if (!t.enabled) return { text: "off", title: `Disabled\n${schedule}`, dim: true };
  if (paused) return { text: "paused", title: `All triggers paused\n${schedule}`, dim: true };
  if (!t.next_fire_at)
    return { text: "—", title: `Not scheduled: ${NOT_SCHEDULED_HINT}\n${schedule}`, dim: true };
  const until = fmtUntil(t.next_fire_at, now);
  return { text: until, title: `${until === "now" ? "Next fire now" : `Next fire in ${until}`}\n${schedule}`, dim: false };
}

/**
 * One Trigger row, rendered identically flat or grouped by Projet (#258): its
 * switch first, its name and its modes (guard, overlap); then its pipeline,
 * its last fire and the next one. Under the switch, its last result, which
 * doubles as the select control.
 *
 * Every element but its switch, its name and its last result is an Élément
 * masquable (`model/triggerRowElements.ts`): one the Apparence hides is not
 * rendered, and the row takes one line when its whole second line is hidden —
 * the last result then sits beside the switch. The Apparence decides, not the
 * Trigger, so every row of the list keeps one height. In a preview of Settings ›
 * Appearance, pointing to an element of a hidden second line brings that line
 * back as a ghost (`data-ghost-line`); a preview's row is inert: no hover bar,
 * out of the tab order.
 */
export default function TriggerRow({
  trigger: t,
  isSelected,
  rowSelected,
  selecting,
  paused = false,
  onSelect,
  onSelectControl,
  onRunNow,
  onEdit,
  onDelete,
  onToggle,
  refused = {},
}: Props) {
  // ADR-0081 (#31): what the `trigger/item` decorations mark — with its last
  // editor and its suspension (API 0.8), for an extension that answers for it.
  const extra = useMemo(() => ({ entity: triggerEntity(t) }), [t]);
  // API 0.8: a suspended Trigger reads switched off, whatever its stored bit.
  const suspension = t.suspension ?? null;
  const on = t.enabled && !suspension;
  const now = useListClock();
  const next = nextOf(t, paused, now);
  const last = lastOutcomeTooltip(t);
  const tone = outcomeTone(t.last_outcome);
  const toggleLabel = suspension
    ? `Suspended: ${suspensionTooltip(suspension)}`
    : t.enabled
      ? "Disable trigger"
      : "Enable trigger";
  const Overlap = t.overlap_policy === "allow" ? Layers : SkipForward;
  const preview = useAppearancePreview();
  const marksOf = useElementMarks();
  // From what the Apparence shows alone: a preview's ghost never keeps a line.
  const appearance = useAppearance();
  const secondLine = hasSecondLine((id) => isVisible(appearance, id));
  const pointed = preview?.pointed ?? null;
  // A preview points to an element of a hidden second line this row can draw.
  const ghostLine =
    !secondLine &&
    pointed !== null &&
    isSecondLineElement(pointed) &&
    (pointed !== TRIGGER_ROW.last || Boolean(t.last_fired_at));
  const pipelineMarks = marksOf(TRIGGER_ROW.pipeline);
  const lastMarks = marksOf(TRIGGER_ROW.last);
  const nextMarks = marksOf(TRIGGER_ROW.next);
  const guardMarks = marksOf(TRIGGER_ROW.guard);
  const overlapMarks = marksOf(TRIGGER_ROW.overlap);
  // #577 — the last result doubles as the select control.
  const lastResult = (
    <StatusMark
      glyph={
        <span data-testid="trigger-status-dot" data-outcome={t.last_outcome ?? "never"}>
          {tone ? (
            <StatusIcon kind="tone" status={tone} label={last} size={12} />
          ) : (
            <Circle size={12} strokeWidth={2} className="lp-dim" role="img" aria-label={last} />
          )}
        </span>
      }
      tone={tone ?? "neutral"}
      selected={rowSelected}
      selecting={selecting}
      label={rowSelected ? `Deselect ${t.name}` : `Select ${t.name}`}
      title={`${last}\nClick to select, Shift+click for a range`}
      onSelect={(e) => onSelectControl(e.shiftKey)}
    />
  );
  const switchButton = (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={t.enabled ? "Disable trigger" : "Enable trigger"}
      aria-disabled={refused.update ? true : undefined}
      title={refused.update ? undefined : toggleLabel}
      className="d-switch aria-disabled:cursor-not-allowed aria-disabled:opacity-45"
      data-testid="trigger-toggle"
      onClick={(e) => {
        e.stopPropagation();
        if (!refused.update) onToggle();
      }}
      onKeyDown={(e) => e.stopPropagation()}
    >
      <span className="d-switch__knob" aria-hidden />
    </button>
  );
  const fireRefusal = fireRefusalOf(t, refused.fire);
  // The menu always holds Enable/Disable and Delete; Run now only when offered.
  const menuReasons = reasonsOf(onRunNow && fireRefusal, refused.update, refused.delete);
  return (
    <div
      role="button"
      tabIndex={preview ? -1 : 0}
      className="lp-row"
      data-lines={secondLine ? 2 : 1}
      data-ghost-line={ghostLine || undefined}
      data-open={isSelected || undefined}
      data-checked={rowSelected || undefined}
      data-off={!t.enabled || undefined}
      data-suspended={suspension ? true : undefined}
      aria-current={isSelected || undefined}
      onClick={onSelect}
      onKeyDown={(e) => {
        if (e.target !== e.currentTarget) return;
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onSelect();
        }
      }}
      data-testid="trigger-row"
      // #911: one row addressable by id, the way `data-run-row` is for Runs.
      data-trigger-row={t.id}
    >
      <span className="lp-lead lp-lead--trigger">
        {/* Enable/disable — always there, so a paused state is reversible in one gesture. */}
        {refused.update ? (
          <Tooltip label={toggleLabel} description={refused.update}>
            {switchButton}
          </Tooltip>
        ) : (
          switchButton
        )}
        {/* On one line, the last result sits beside the switch. */}
        {!secondLine && lastResult}
      </span>
      <span className="lp-l1">
        <span className="lp-name" title={t.name} data-testid="trigger-name">
          {t.name}
        </span>
        {suspension && <SuspendedPill suspension={suspension} />}
        <span className="lp-end">
          {/* The `trigger/item` decorations (ADR-0081, #31); nothing without one. */}
          <span className="lp-deco">
            <ExtensionDecorations<DecorationContext> location="trigger/item" extra={extra} nowrap />
          </span>
          {((t.guard_command && guardMarks) || overlapMarks) && (
            <span className="lp-modes">
              {t.guard_command && guardMarks && (
                <Tooltip label={`Guard: ${t.guard_command}`}>
                  <span role="img" aria-label="Guard" data-testid="trigger-guard" {...guardMarks}>
                    <ShieldQuestion size={12} strokeWidth={1.5} aria-hidden />
                  </span>
                </Tooltip>
              )}
              {overlapMarks && (
                <Tooltip label={`Overlap: ${overlapLabel(t)}`}>
                  <span
                    role="img"
                    aria-label={`Overlap: ${t.overlap_policy}`}
                    data-testid="trigger-overlap"
                    {...overlapMarks}
                  >
                    <Overlap size={12} strokeWidth={1.5} aria-hidden />
                  </span>
                </Tooltip>
              )}
            </span>
          )}
        </span>
      </span>
      {secondLine && <span className="lp-lead lp-lead--l2">{lastResult}</span>}
      {(secondLine || ghostLine) && (
        <span className="lp-l2">
          {pipelineMarks && (
            <span className="lp-pipe" title={t.pipeline_name} data-testid="trigger-pipeline" {...pipelineMarks}>
              {t.pipeline_name}
            </span>
          )}
          <span className="lp-end">
            {t.last_fired_at && lastMarks && (
              <span className="lp-num lp-muted" title={last} data-testid="trigger-last-fired" {...lastMarks}>
                {fmtAge(t.last_fired_at, now)}
              </span>
            )}
            {nextMarks && (
              <span className="lp-modes" title={next.title} data-testid="trigger-schedule" {...nextMarks}>
                <CalendarClock size={11} strokeWidth={1.75} aria-hidden />
                <span className={next.dim ? "lp-num lp-muted" : "lp-num"}>{next.text}</span>
              </span>
            )}
          </span>
        </span>
      )}
      {!preview && (
      <HoverBar testId="trigger-row-actions">
        {onRunNow && (
          <RowAction icon={Play} label="Run now" testId="trigger-run-now" onClick={onRunNow} disabledReason={fireRefusal} />
        )}
        {onEdit && (
          <RowAction icon={Pencil} label="Edit trigger" testId="trigger-edit" onClick={onEdit} disabledReason={refused.update} />
        )}
        <RowAction
          icon={Trash2}
          label="Delete trigger"
          danger
          testId="trigger-delete"
          onClick={onDelete}
          disabledReason={refused.delete}
        />
        {/* #30 — the « ⋯ » menu: the row's verbs, the extensions'
            `trigger/item/menu` groups, then Delete. */}
        <RowMenu label={`More actions for ${t.name}`} testId="trigger-row-menu" reasons={menuReasons}>
          {onRunNow && (
            <RowMenuItem data-testid="trigger-menu-run-now" disabledReason={fireRefusal} onClick={onRunNow}>
              <Play /> Run now
            </RowMenuItem>
          )}
          {onEdit && (
            <RowMenuItem data-testid="trigger-menu-edit" disabledReason={refused.update} onClick={onEdit}>
              <Pencil /> Edit
            </RowMenuItem>
          )}
          <RowMenuItem data-testid="trigger-menu-toggle" disabledReason={refused.update} onClick={onToggle}>
            <Power /> {t.enabled ? "Disable" : "Enable"}
          </RowMenuItem>
          <RowMenuItem data-testid="trigger-menu-copy-id" onClick={() => copyTriggerId(t.id)}>
            <Copy /> Copy trigger id
          </RowMenuItem>
          <ExtensionMenuGroups location="trigger/item/menu" extra={extra} itemClassName={ROW_MENU_EXTENSION_ITEM} />
          <DropdownMenuSeparator />
          <RowMenuItem variant="destructive" data-testid="trigger-menu-delete" disabledReason={refused.delete} onClick={onDelete}>
            <Trash2 /> Delete
          </RowMenuItem>
        </RowMenu>
      </HoverBar>
      )}
    </div>
  );
}

/** A row's Trigger, as its menu's context hands it over. */
function triggerEntity(t: Trigger): TriggerEntity {
  return {
    kind: "trigger",
    id: t.id,
    name: t.name,
    pipeline: t.pipeline_name,
    enabled: t.enabled,
    attributes: t.attributes ?? {},
    last_editor: t.last_editor ?? null,
    suspension: t.suspension ?? null,
  };
}

function copyTriggerId(id: string) {
  void copyToClipboard(id).then((ok) =>
    ok ? toast("Trigger id copied") : toast("The clipboard is not available", "error"),
  );
}
