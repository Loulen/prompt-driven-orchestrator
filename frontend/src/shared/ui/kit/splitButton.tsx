/**
 * The split button of the shared design system: a main action welded to the
 * chevron of its variants' menu. Styles in kit.css (`d-split`).
 */
import * as React from "react";
import { ChevronDown } from "lucide-react";
import { Button } from "./controls";
import { Menu, type MenuEntry } from "./menu";
import { Tooltip } from "./tip";
import type { Icon } from "./tones";
import { cx } from "./util";

export interface SplitButtonProps {
  /** The main action's text. */
  children: React.ReactNode;
  icon?: Icon;
  /** primary: the one main action of a view (ruby). */
  tone?: "default" | "primary";
  /** 24 px (`md`) or 20 px (`sm`). */
  size?: "md" | "sm";
  onClick?: () => void;
  /**
   * Why the caller may not use the main action (their rights): greyed as
   * `Button` greys, its tooltip says this sentence. Wins over `pendingReason`.
   */
  disabledReason?: string;
  /**
   * Why the main action cannot run yet (something is not there): inert
   * (`aria-disabled`, still focusable), drawn in a lighter tone rather than
   * greyed, its tooltip says this sentence. The chevron stays live.
   */
  pendingReason?: string;
  /** The menu under the chevron; without an entry, a plain `Button`. */
  entries?: MenuEntry[];
  /** Accessible name of the chevron and of its menu. */
  menuLabel?: string;
  /** The chevron's tooltip, when it says more than `menuLabel` (a state). */
  chevronTip?: string;
  /** A dot on the chevron: something the menu set is on. */
  indicator?: boolean;
  /** `data-testid` of the main button; the chevron takes `<id>-menu`. */
  "data-testid"?: string;
  className?: string;
}

/**
 * A main action welded to a chevron that opens a `Menu` of its variants: one
 * piece, a hairline between the two. Both are focusable; the menu runs on the
 * keyboard as `Menu` does. Without entries, a plain `Button`. The ref reaches
 * the main button.
 */
export const SplitButton = React.forwardRef<HTMLButtonElement, SplitButtonProps>(function SplitButton(
  {
    children,
    icon,
    tone = "default",
    size = "md",
    onClick,
    disabledReason,
    pendingReason,
    entries = [],
    menuLabel = "More actions",
    chevronTip,
    indicator = false,
    "data-testid": testId,
    className,
  },
  ref,
) {
  const pending = !disabledReason && !!pendingReason;
  const main = (
    <Button
      ref={ref}
      icon={icon}
      tone={tone}
      size={size}
      data-testid={testId}
      data-pending={pending || undefined}
      className={entries.length > 0 ? "d-split__main" : undefined}
      disabledReason={disabledReason}
      {...(pending
        ? {
            "aria-disabled": true,
            onClick: (e: React.MouseEvent) => e.preventDefault(),
            onKeyDown: (e: React.KeyboardEvent) => {
              if (e.key === "Enter" || e.key === " ") e.preventDefault();
            },
          }
        : { onClick })}
    >
      {children}
    </Button>
  );
  // Always wrapped, so the button keeps its node (and the focus) when it turns pending.
  const mainWithTip = (
    <Tooltip label={pendingReason ?? ""} disabled={!pending}>
      {main}
    </Tooltip>
  );
  if (entries.length === 0) return mainWithTip;
  return (
    <span
      className={cx("d-split", tone === "primary" && "d-split--primary", size === "sm" && "d-split--sm", className)}
      data-pending={pending || undefined}
    >
      {mainWithTip}
      <Menu
        label={menuLabel}
        align="end"
        entries={entries}
        trigger={({ open, toggle }) => (
          <Tooltip label={chevronTip ?? menuLabel}>
            <button
              type="button"
              className={cx("d-btn", tone === "primary" && "d-btn--primary", size === "sm" && "d-btn--sm", "d-split__chev")}
              aria-label={menuLabel}
              aria-haspopup="menu"
              aria-expanded={open}
              data-testid={testId ? `${testId}-menu` : undefined}
              data-indicator={indicator || undefined}
              onClick={toggle}
              onKeyDown={(e) => {
                if (!open && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
                  e.preventDefault();
                  toggle();
                }
              }}
            >
              <ChevronDown size={size === "sm" ? 12 : 14} strokeWidth={1.5} aria-hidden />
              {indicator && <span className="d-split__dot" aria-hidden />}
            </button>
          </Tooltip>
        )}
      />
    </span>
  );
});
