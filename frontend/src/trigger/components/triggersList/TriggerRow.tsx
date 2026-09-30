import { useMemo } from "react";
import { Pencil, Play, Power, Trash2 } from "lucide-react";
import ExtensionDecorations from "../../../shared/extensions/ExtensionDecorations";
import type { DecorationContext } from "../../../shared/extensions/contract";
import type { Trigger } from "../../../shared/generated/Trigger";
import { repoGroupLabel } from "../../../lib/groupByRepo";
import SelectControl from "../../../shared/ui/SelectControl";
import { humanizeCron } from "../../model/cronPresets";
import { lastOutcomeTooltip, outcomeDot, outcomeRing } from "../../model/outcome";

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
  // ADR-0081 (#31): what the `trigger/item` decorations mark.
  const decorated = useMemo(
    () => ({ entity: { kind: "trigger", id: t.id, name: t.name, attributes: t.attributes ?? {} } }),
    [t.id, t.name, t.attributes],
  );
  return (
    <button
      onClick={onSelect}
      className={`group flex w-full cursor-pointer items-center gap-2 border-b border-l-2 border-line-soft px-3 py-2 text-left transition-colors ${
        rowSelected
          ? "border-l-acc bg-acc-bg text-fg"
          : isSelected
            ? "border-l-transparent bg-bg-3 text-fg"
            : "border-l-transparent text-fg-2 hover:bg-bg-3/50"
      } ${t.enabled ? "" : "opacity-60"}`}
      style={{ fontSize: "11.5px" }}
      data-testid="trigger-row"
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
        <div className="truncate font-medium">{t.name}</div>
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
          extra={decorated}
          block
          className="mt-0.5"
        />
      </div>
      {!t.enabled && (
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
      {/* Enable/disable toggle — always visible so paused state is reversible. */}
      <span
        role="switch"
        aria-checked={t.enabled}
        aria-label={t.enabled ? "Disable trigger" : "Enable trigger"}
        title={t.enabled ? "Disable trigger" : "Enable trigger"}
        className={`shrink-0 cursor-pointer rounded p-0.5 transition-colors hover:bg-bg-4 ${
          t.enabled ? "text-acc" : "text-fg-4"
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
