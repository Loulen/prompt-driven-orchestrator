import type { ReactNode } from "react";
import { LoaderCircle, X } from "lucide-react";
import { Tooltip } from "../../shared/ui/kit";

export interface BulkAction {
  key: string;
  label: string;
  icon?: ReactNode;
  /** Red on hover for a destructive action (Cleanup / Delete). */
  destructive?: boolean;
  /** Greyed + non-clickable when no selected item is a valid target. */
  disabled?: boolean;
  /**
   * Why the caller may make it on none of the selected items (CORE-205):
   * greyed (`aria-disabled`), inert, its tooltip saying this sentence.
   */
  disabledReason?: string;
  onClick: () => void;
}

interface Props {
  count: number;
  /** Selected items that are live, with what the actions do to them ("2 running will stop"). */
  live?: { count: number; note: string } | null;
  actions: BulkAction[];
  onClear: () => void;
  /** `pane`: over the foot of its list (the left panel). `viewport`: over the whole window. */
  placement?: "pane" | "viewport";
  /** The actions' names next to their icons (a bar of one or two actions). */
  labelled?: boolean;
}

/**
 * The floating bar of the multi-select (#577): a 28 px pill with the count,
 * the live ones, the valid actions as icons named in their tooltips, and
 * Clear. Purely presentational: each caller (Runs / Triggers / Pipelines)
 * owns which actions it passes and what they do.
 */
export default function BulkActionBar({ count, live, actions, onClear, placement = "pane", labelled }: Props) {
  return (
    <div
      role="toolbar"
      aria-label="Bulk actions"
      data-testid="bulk-action-bar"
      className="lp-bulk"
      style={placement === "viewport" ? { position: "fixed", bottom: "var(--g-7)" } : undefined}
    >
      <span className="lp-bulk__count" data-testid="bulk-count">
        <span className="lp-num">{count}</span> <span className="d-sr">selected</span>
        {live && live.count > 0 && (
          <Tooltip label={live.note}>
            <span className="lp-bulk__live" data-testid="bulk-note" aria-label={live.note}>
              <LoaderCircle size={11} strokeWidth={2} aria-hidden />
              {live.count}
            </span>
          </Tooltip>
        )}
      </span>
      {actions.length > 0 && <span className="lp-bulk__sep" aria-hidden="true" />}
      <span className="lp-bulk__acts">
        {actions.map((a) => (
          labelled ? (
            <Tooltip key={a.key} label={a.label} description={a.disabledReason} disabled={!a.disabledReason}>
              <button
                type="button"
                disabled={a.disabled && !a.disabledReason}
                aria-disabled={a.disabledReason ? true : undefined}
                onClick={a.disabledReason ? undefined : a.onClick}
                data-testid={`bulk-action-${a.key}`}
                className={`d-btn d-btn--ghost d-btn--sm${a.destructive ? " d-btn--danger" : ""}`}
              >
                {a.icon}
                {a.label}
              </button>
            </Tooltip>
          ) : (
            <Tooltip key={a.key} label={a.label} description={a.disabledReason}>
              <button
                type="button"
                disabled={a.disabled && !a.disabledReason}
                aria-disabled={a.disabledReason ? true : undefined}
                onClick={a.disabledReason ? undefined : a.onClick}
                aria-label={a.label}
                data-testid={`bulk-action-${a.key}`}
                className={`d-icon${a.destructive ? " d-icon--danger" : ""}`}
              >
                {a.icon}
              </button>
            </Tooltip>
          )
        ))}
      </span>
      <span className="lp-bulk__sep" aria-hidden="true" />
      <Tooltip label="Clear selection">
        <button type="button" onClick={onClear} aria-label="Clear selection" data-testid="bulk-clear" className="d-icon">
          <X size={14} strokeWidth={1.5} aria-hidden />
        </button>
      </Tooltip>
    </div>
  );
}
