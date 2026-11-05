import { Info, X } from "lucide-react";

/**
 * One banner row. `kind` is the single source of truth for dismissibility
 * (#268): a `nudge` is an advisory fan-out suggestion that the user may dismiss
 * persistently; `lint` is a correctness diagnostic (silent config loss, dangling
 * edges) that stays always-visible per ADR-0001 — the off-switch is for advisory
 * signals, not correctness ones.
 */
export interface LintBannerItem {
  /**
   * Stable identity. lint → `lint:<index>` (the daemon flattens diagnostics to
   * bare strings with no id). nudge → `fanout:<targetNodeId>` (rename-stable).
   */
  id: string;
  kind: "lint" | "nudge";
  message: string;
  /**
   * ADR-0081 / gitlab #23 — the node the row is about, when it names one (a
   * Cul-de-sac, a Nœud orphelin). The row then offers a quiet « Show » link that
   * selects and centres that node. Absent ⇒ no link (e.g. « no path »).
   */
  nodeId?: string;
}

interface Props {
  items: LintBannerItem[];
  /** Called only for `nudge` rows (lint rows render no dismiss affordance). */
  onDismiss: (id: string) => void;
  /** « Show » on a row naming a node. Without it, no row renders the link. */
  onShow?: (nodeId: string) => void;
  /**
   * #1022: the node currently selected on the canvas. The « Show » of a row
   * naming it stays lit (accent + underline), so the clicked link reads as the
   * one in effect instead of falling back to grey.
   */
  activeNodeId?: string | null;
}

export default function LintBanner({ items, onDismiss, onShow, activeNodeId }: Props) {
  if (items.length === 0) return null;

  return (
    <div
      data-testid="lint-banner"
      className="mx-3 mt-2 flex flex-col gap-1"
    >
      {items.map((item, i) => (
        <div
          key={item.kind === "nudge" ? item.id : `lint-${i}`}
          data-testid="lint-banner-row"
          // #1024: the amber wash sits on an OPAQUE base (`bg-bg-1` under a
          // flat gradient of the tint) — the bare /5 tint let the nodes under
          // the banner show through.
          className="flex items-start gap-2 rounded border border-st-await/30 bg-bg-1 bg-linear-to-r from-st-await/5 to-st-await/5 px-2 py-1.5 text-fg-3"
          style={{ fontSize: "10.5px" }}
        >
          <Info size={12} className="mt-0.5 shrink-0 text-st-await" />
          <span>{item.message}</span>
          {item.nodeId != null && onShow && (
            <button
              type="button"
              onClick={() => onShow(item.nodeId!)}
              data-testid={`lint-banner-show-${item.nodeId}`}
              aria-current={item.nodeId === activeNodeId ? "true" : undefined}
              className={`ml-auto shrink-0 hover:text-acc hover:underline focus-visible:outline focus-visible:outline-1 ${
                item.nodeId === activeNodeId ? "text-acc underline" : "text-fg-4"
              }`}
            >
              Show
            </button>
          )}
          {item.kind === "nudge" && (
            <button
              type="button"
              onClick={() => onDismiss(item.id)}
              aria-label="Dismiss suggestion"
              data-testid={`lint-banner-dismiss-${item.id}`}
              className="ml-auto -mr-0.5 shrink-0 rounded p-0.5 text-fg-4 hover:text-fg-2 focus-visible:outline focus-visible:outline-1"
            >
              <X size={12} />
            </button>
          )}
        </div>
      ))}
    </div>
  );
}
