import { useMemo } from "react";
import { Copy, Pencil, Play, Power, Trash2 } from "lucide-react";
import ExtensionDecorations from "../../../shared/extensions/ExtensionDecorations";
import { ExtensionMenuGroups } from "../../../shared/extensions/ExtensionActions";
import type { DecorationContext, TriggerEntity } from "../../../shared/extensions/contract";
import { DropdownMenuSeparator } from "../../../shared/ui/dropdown-menu";
import RowMenu, { ROW_MENU_EXTENSION_ITEM, RowMenuItem } from "../../../shared/ui/RowMenu";
import { toast } from "../../../shared/ui/toast";
import type { Trigger } from "../../../shared/generated/Trigger";
import { repoGroupLabel } from "../../../lib/groupByRepo";
import SelectControl from "../../../shared/ui/SelectControl";
import { humanizeCron } from "../../model/cronPresets";
import { lastOutcomeTooltip, outcomeDot, outcomeRing } from "../../model/outcome";
import SuspendedPill from "./SuspendedPill";
import { suspensionTooltip } from "../../model/suspension";
import { copyToClipboard } from "../../../shared/lib/clipboard";

interface Props {
  trigger: Trigger;
  /** The Trigger the right panel shows. */
  isSelected: boolean;
  /** Multi-selected (#577). */
  rowSelected: boolean;
  /** Every raw target repo of the list, so a colliding basename is
   *  disambiguated identically in the badge and the group header (#258 G7). */
  allRepos: string[];
  onSelect: () => void;
  /** The multi-select control was clicked (shift ⇒ a range). */
  onSelectControl: (shiftKey: boolean) => void;
  onRunNow?: () => void;
  onEdit?: () => void;
  onDelete: () => void;
  onToggle: () => void;
}

/** One Trigger row, rendered identically flat or grouped by repo (#258). */
export default function TriggerRow({
  trigger: t,
  isSelected,
  rowSelected,
  allRepos,
  onSelect,
  onSelectControl,
  onRunNow,
  onEdit,
  onDelete,
  onToggle,
}: Props) {
  const dot = outcomeDot(t.last_outcome);
  // ADR-0081 (#31): what the `trigger/item` decorations mark — with its last
  // editor and its suspension (API 0.8), for an extension that answers for it.
  const extra = useMemo(() => ({ entity: triggerEntity(t) }), [t]);
  // API 0.8: a suspended Trigger reads switched off, whatever its stored bit.
  const suspension = t.suspension ?? null;
  const on = t.enabled && !suspension;
  return (
    <button
      onClick={onSelect}
      className={`group flex w-full cursor-pointer items-center gap-2 border-b border-l-2 border-line-soft px-3 py-2 text-left transition-colors ${
        rowSelected
          ? "border-l-acc bg-acc-bg text-fg"
          : isSelected
            ? "border-l-transparent bg-bg-3 text-fg"
            : "border-l-transparent text-fg-2 hover:bg-bg-3/50"
      } ${suspension ? "bg-st-interrupted/5" : ""} ${t.enabled ? "" : "opacity-60"}`}
      style={{ fontSize: "11.5px" }}
      data-testid="trigger-row"
      // #911: one row addressable by id, the way `data-run-row` is for Runs.
      data-trigger-row={t.id}
    >
      <SelectControl
        selected={rowSelected}
        dotClass={dot}
        ringClass={outcomeRing(t.last_outcome)}
        dotTitle={lastOutcomeTooltip(t)}
        dotTestId="trigger-status-dot"
        label={rowSelected ? `Deselect ${t.name}` : `Select ${t.name}`}
        onSelect={(e) => onSelectControl(e.shiftKey)}
      />
      <div className="min-w-0 flex-1">
        {/* The « suspended » pill wraps under the name rather than eat it in a
            narrow panel (or once the hover actions take their room). */}
        <div className="flex min-w-0 flex-wrap items-center gap-x-1.5 gap-y-0.5" data-testid="trigger-name-line">
          <span className={`max-w-full truncate font-medium ${suspension ? "text-fg-3" : ""}`}>{t.name}</span>
          {suspension && <SuspendedPill suspension={suspension} />}
        </div>
        <div
          className="mt-0.5 flex items-center gap-1.5 truncate text-fg-4"
          style={{ fontSize: "10px" }}
        >
          <span className="truncate font-mono">{t.pipeline_name}</span>
          {t.target_repo && (
            <span
              className="shrink-0 rounded border border-line-strong px-1 py-px"
              style={{ fontSize: "9px" }}
              title={t.target_repo}
            >
              {repoGroupLabel(t.target_repo, allRepos)}
            </span>
          )}
          <span className="shrink-0" aria-hidden="true">·</span>
          <span className="shrink-0" data-testid="trigger-schedule">
            {humanizeCron(t.cron)}
          </span>
        </div>
        {/* Line 3: the `trigger/item` decorations (ADR-0081, #31). A line of
            their own, as on a Run row, so a badge never truncates the
            pipeline's name and several wrap; nothing at all without one. */}
        <ExtensionDecorations<DecorationContext>
          location="trigger/item"
          extra={extra}
          block
          className="mt-0.5"
        />
      </div>
      {!t.enabled && !suspension && (
        <span
          className="shrink-0 rounded border border-line-strong px-1 py-px text-fg-4"
          style={{ fontSize: "9px" }}
        >
          disabled
        </span>
      )}
      {/* Hover actions: run-now, edit, delete. */}
      <span className="hidden shrink-0 items-center gap-0.5 group-hover:inline-flex">
        <span
          role="button"
          title="Run now (test this trigger)"
          className="cursor-pointer rounded p-0.5 text-fg-4 transition-colors hover:bg-bg-4 hover:text-acc"
          onClick={(e) => {
            e.stopPropagation();
            onRunNow?.();
          }}
          data-testid="trigger-run-now"
        >
          <Play size={12} />
        </span>
        <span
          role="button"
          title="Edit trigger"
          className="cursor-pointer rounded p-0.5 text-fg-4 transition-colors hover:bg-bg-4 hover:text-fg"
          onClick={(e) => {
            e.stopPropagation();
            onEdit?.();
          }}
          data-testid="trigger-edit"
        >
          <Pencil size={12} />
        </span>
        <span
          role="button"
          title="Delete trigger"
          className="cursor-pointer rounded p-0.5 text-fg-4 transition-colors hover:bg-bg-4 hover:text-st-failed"
          onClick={(e) => {
            e.stopPropagation();
            onDelete();
          }}
          data-testid="trigger-delete"
        >
          <Trash2 size={12} />
        </span>
      </span>
      {/* #30 — the « ⋯ » menu: the row's verbs, the extensions'
          `trigger/item/menu` groups, then Delete. */}
      <RowMenu label={`More actions for ${t.name}`} testId="trigger-row-menu">
        {onRunNow && (
          <RowMenuItem data-testid="trigger-menu-run-now" onClick={onRunNow}>
            <Play /> Run now
          </RowMenuItem>
        )}
        {onEdit && (
          <RowMenuItem data-testid="trigger-menu-edit" onClick={onEdit}>
            <Pencil /> Edit
          </RowMenuItem>
        )}
        <RowMenuItem data-testid="trigger-menu-toggle" onClick={onToggle}>
          <Power /> {t.enabled ? "Disable" : "Enable"}
        </RowMenuItem>
        <RowMenuItem data-testid="trigger-menu-copy-id" onClick={() => copyTriggerId(t.id)}>
          <Copy /> Copy trigger id
        </RowMenuItem>
        <ExtensionMenuGroups location="trigger/item/menu" extra={extra} itemClassName={ROW_MENU_EXTENSION_ITEM} />
        <DropdownMenuSeparator />
        <RowMenuItem variant="destructive" data-testid="trigger-menu-delete" onClick={onDelete}>
          <Trash2 /> Delete
        </RowMenuItem>
      </RowMenu>
      {/* Enable/disable toggle — always visible so paused state is reversible. */}
      <span
        role="switch"
        aria-checked={on}
        aria-label={t.enabled ? "Disable trigger" : "Enable trigger"}
        title={
          suspension
            ? `Suspended: ${suspensionTooltip(suspension)}`
            : t.enabled
              ? "Disable trigger"
              : "Enable trigger"
        }
        className={`shrink-0 cursor-pointer rounded p-0.5 transition-colors hover:bg-bg-4 ${
          on ? "text-acc" : "text-fg-4"
        }`}
        onClick={(e) => {
          e.stopPropagation();
          onToggle();
        }}
        data-testid="trigger-toggle"
      >
        <Power size={12} />
      </span>
    </button>
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
