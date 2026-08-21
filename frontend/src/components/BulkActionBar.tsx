import type { ReactNode } from "react";

export interface BulkAction {
  key: string;
  label: string;
  icon?: ReactNode;
  /** Red styling for a destructive action (Cleanup / Delete). */
  destructive?: boolean;
  /** Greyed + non-clickable when no selected item is a valid target. */
  disabled?: boolean;
  onClick: () => void;
}

interface Props {
  count: number;
  /** Domain-aware caveat, e.g. "1 running will stop" — omitted when not relevant. */
  note?: string | null;
  actions: BulkAction[];
  onClear: () => void;
}

/**
 * The floating, bottom-centre action bar of the multi-select feature (#577,
 * design "D"). Purely presentational: it renders the count, a domain-aware note,
 * the valid bulk actions and Clear — each caller (Runs / Triggers / Library)
 * owns which actions it passes and what they do. `fixed` + centred so it hovers
 * over the whole workspace, escaping the left panel's own scroll/overflow.
 */
export default function BulkActionBar({ count, note, actions, onClear }: Props) {
  return (
    <div
      role="toolbar"
      aria-label="Bulk actions"
      data-testid="bulk-action-bar"
      className="fixed bottom-6 left-1/2 z-40 flex -translate-x-1/2 items-center gap-4 rounded-xl border border-line-strong bg-bg-4/95 px-4 py-2.5 shadow-lg backdrop-blur"
    >
      <div className="flex items-baseline gap-2">
        <span className="font-medium text-fg" style={{ fontSize: "13px" }} data-testid="bulk-count">
          {count} selected
        </span>
        {note && (
          <span
            className="leading-tight text-fg-4"
            style={{ fontSize: "10.5px", maxWidth: "88px" }}
            data-testid="bulk-note"
          >
            {note}
          </span>
        )}
      </div>

      <div className="h-5 w-px shrink-0 bg-line-strong" />

      <div className="flex items-center gap-1">
        {actions.map((a) => (
          <button
            key={a.key}
            type="button"
            disabled={a.disabled}
            onClick={a.onClick}
            data-testid={`bulk-action-${a.key}`}
            className={`flex cursor-pointer items-center gap-1.5 rounded-md px-2.5 py-1 font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-40 ${
              a.destructive
                ? "text-st-failed hover:bg-st-failed/10"
                : "text-fg-2 hover:bg-bg-5 hover:text-fg"
            }`}
            style={{ fontSize: "12px" }}
          >
            {a.icon}
            {a.label}
          </button>
        ))}
      </div>

      <div className="h-5 w-px shrink-0 bg-line-strong" />

      <button
        type="button"
        onClick={onClear}
        data-testid="bulk-clear"
        className="cursor-pointer text-fg-3 transition-colors hover:text-fg"
        style={{ fontSize: "12px" }}
      >
        Clear
      </button>
    </div>
  );
}
