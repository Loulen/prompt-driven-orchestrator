import { useState } from "react";
import { ChevronDown, ChevronUp, Lightbulb, TriangleAlert, X } from "lucide-react";
import { Button, IconButton } from "../../shared/ui/kit";

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
}

interface Props {
  items: LintBannerItem[];
  /** Called only for `nudge` rows (lint rows render no dismiss affordance). */
  onDismiss: (id: string) => void;
}

function LintIcon({ kind }: { kind: LintBannerItem["kind"] }) {
  return kind === "lint" ? (
    <TriangleAlert size={13} strokeWidth={1.5} aria-label="Warning" role="img" />
  ) : (
    <Lightbulb size={13} strokeWidth={1.5} aria-label="Suggestion" role="img" />
  );
}

function Dismiss({ item, onDismiss }: { item: LintBannerItem; onDismiss: Props["onDismiss"] }) {
  if (item.kind !== "nudge") return null;
  return (
    <IconButton
      icon={X}
      size="sm"
      label="Dismiss suggestion"
      data-testid={`lint-banner-dismiss-${item.id}`}
      onClick={() => onDismiss(item.id)}
    />
  );
}

/**
 * One 24 px line under the tab bar: the icon, the count, the first issue;
 * « Show » unfolds the others, one line each.
 */
export default function LintBanner({ items, onDismiss }: Props) {
  const [open, setOpen] = useState(false);
  if (items.length === 0) return null;
  const [first, ...rest] = items;
  const kind = items.some((i) => i.kind === "lint") ? "lint" : "nudge";
  return (
    <div data-testid="lint-banner" role="status">
      <div className="cv-line" data-kind={kind}>
        <LintIcon kind={kind} />
        <span className="cv-line__count">{items.length}</span>
        <span className="cv-line__msg" title={first.message}>
          {first.message}
        </span>
        <span className="cv-line__actions">
          {rest.length > 0 && (
            <Button
              tone="ghost"
              size="sm"
              aria-expanded={open}
              data-testid="lint-banner-toggle"
              onClick={() => setOpen((o) => !o)}
            >
              {open ? "Hide" : "Show"}
              {open ? <ChevronUp size={12} strokeWidth={1.5} aria-hidden /> : <ChevronDown size={12} strokeWidth={1.5} aria-hidden />}
            </Button>
          )}
          <Dismiss item={first} onDismiss={onDismiss} />
        </span>
      </div>
      {open &&
        rest.map((item, i) => (
          <div key={item.kind === "nudge" ? item.id : `lint-${i}`} className="cv-line cv-line--sub" data-kind={item.kind}>
            <LintIcon kind={item.kind} />
            <span className="cv-line__msg" title={item.message}>
              {item.message}
            </span>
            <span className="cv-line__actions">
              <Dismiss item={item} onDismiss={onDismiss} />
            </span>
          </div>
        ))}
    </div>
  );
}
