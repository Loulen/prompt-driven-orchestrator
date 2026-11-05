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
}

export default function LintBanner({ items, onDismiss, onShow }: Props) {
  if (items.length === 0) return null;

  return (
    <div
      data-testid="lint-banner"
      className="mx-3 mt-2 flex flex-col gap-1"
    >
      {items.map((item, i) => (
        <div
          key={item.kind === "nudge" ? item.id : `lint-${i}`}
          className="flex items-start gap-2 rounded border border-st-await/30 bg-st-await/5 px-2 py-1.5 text-fg-3"
          style={{ fontSize: "10.5px" }}
        >
          <Info size={12} className="mt-0.5 shrink-0 text-st-await" />
          <span>{item.message}</span>
          {item.nodeId != null && onShow && (
            <button
              type="button"
              onClick={() => onShow(item.nodeId!)}
              data-testid={`lint-banner-show-${item.nodeId}`}
              className="ml-auto shrink-0 text-fg-4 hover:text-acc hover:underline focus-visible:outline focus-visible:outline-1"
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
