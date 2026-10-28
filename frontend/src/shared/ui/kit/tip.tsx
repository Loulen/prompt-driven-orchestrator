/**
 * Kbd and Tooltip of the shared design system (ADR-0091), and the tooltip layer
 * the core's own tooltip draws with.
 */
import * as React from "react";
import { Floating, type Side } from "./layer";
import { tip } from "./util";

// ================================================================== key

export interface KbdProps {
  children: React.ReactNode;
  title?: string;
}

/** A key or a chord: hairline, no fill, mono. */
export function Kbd({ children, title }: KbdProps) {
  return (
    <kbd className="d-kbd" title={title}>
      {children}
    </kbd>
  );
}

// ================================================================== tooltip

/** Delay before a tooltip shows. */
const TOOLTIP_DELAY = 300;
/** Within this time after a tooltip closed, the next shows at once (along a toolbar). */
const TOOLTIP_SKIP = 300;
let lastTipClose = 0;

/** First element with a box under `el` (the wrapper is `display: contents`). */
function firstBox(el: Element | null): HTMLElement | null {
  let c = el?.firstElementChild ?? null;
  while (c && getComputedStyle(c).display === "contents") c = c.firstElementChild;
  return c as HTMLElement | null;
}

/**
 * The bubble next to one control. Shows after `delay` on hover or on keyboard
 * focus, at once when another closed less than 300 ms ago; hides on leave,
 * blur, pointer-down and Escape.
 */
export function TooltipLayer({
  content,
  side = "bottom",
  delay = TOOLTIP_DELAY,
  disabled,
  describedBy,
  children,
}: {
  content: React.ReactNode;
  side?: Side;
  delay?: number;
  disabled?: boolean;
  /** Id given to the bubble, for the trigger's `aria-describedby`. */
  describedBy?: string;
  children: React.ReactNode;
}) {
  const wrap = React.useRef<HTMLSpanElement>(null);
  const timer = React.useRef(0);
  const pressed = React.useRef(false);
  const [anchor, setAnchor] = React.useState<HTMLElement | null>(null);
  const show = () => {
    if (disabled) return;
    window.clearTimeout(timer.current);
    const now = Date.now() - lastTipClose < TOOLTIP_SKIP;
    timer.current = window.setTimeout(() => setAnchor(firstBox(wrap.current)), now ? 0 : delay);
  };
  const hide = () => {
    window.clearTimeout(timer.current);
    setAnchor((a) => {
      if (a) lastTipClose = Date.now();
      return null;
    });
  };
  React.useEffect(() => () => window.clearTimeout(timer.current), []);
  React.useEffect(() => {
    if (!anchor) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") hide();
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, [anchor]);
  return (
    <span
      ref={wrap}
      style={{ display: "contents" }}
      onPointerEnter={show}
      onPointerLeave={hide}
      onPointerDown={() => {
        pressed.current = true;
        hide();
      }}
      onFocus={() => {
        // A click focuses too: only a keyboard focus opens it.
        if (!pressed.current) show();
        pressed.current = false;
      }}
      onBlur={hide}
    >
      {children}
      <Floating
        anchor={anchor}
        open={!!anchor && !disabled}
        side={side}
        align="center"
        className="d-tiplayer"
        role="tooltip"
        id={describedBy}
      >
        <span className="d-tip" data-testid="tooltip-content">
          {content}
        </span>
      </Floating>
    </span>
  );
}

export interface TooltipProps {
  /** The control's name. */
  label: string;
  kbd?: string;
  /** One short line when the name is not enough (a disabled reason, a count). */
  description?: string;
  side?: "bottom" | "top";
  delay?: number;
  disabled?: boolean;
  /** One control; it keeps its own `aria-label`. */
  children: React.ReactElement;
}

/** Wraps one control: its name and shortcut as text ("Retry (R)"), and the extra line under it. */
export function Tooltip({ label, kbd, description, side, delay, disabled, children }: TooltipProps) {
  const id = React.useId();
  const child = description
    ? React.cloneElement(children as React.ReactElement<{ "aria-describedby"?: string }>, { "aria-describedby": id })
    : children;
  return (
    <TooltipLayer
      side={side}
      delay={delay}
      disabled={disabled}
      describedBy={description ? id : undefined}
      content={
        <>
          <span>{tip(label, kbd)}</span>
          {description && <span className="d-tip__desc">{description}</span>}
        </>
      }
    >
      {child}
    </TooltipLayer>
  );
}
