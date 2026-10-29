/**
 * The floating layers of the kit: a tooltip or a submenu next to its anchor
 * (`Floating`), a panel under its trigger that closes itself (`AnchoredPanel`).
 *
 * Both are portalled into the anchor's open `<dialog>` (everything outside a
 * modal dialog is inert) or into the app's root, and shown in the top layer
 * (`popover="manual"`) when there is one, so they stay above modal dialogs. They are placed
 * from screen rects and written in CSS pixels (`toCss`): the app is drawn
 * with the UI scale's zoom.
 */
import * as React from "react";
import { createPortal } from "react-dom";
import { toCss } from "../zoom";

const MARGIN = 8;

export const FOCUSABLE =
  'button:not([disabled]), [href], input:not([disabled]), select, textarea, [tabindex]:not([tabindex="-1"])';

/** Where a floating layer of `anchor` is mounted. */
function hostOf(anchor: Element | null): HTMLElement {
  return ((anchor?.closest("dialog[open]") as HTMLElement | null) ??
    document.getElementById("root") ??
    document.body) as HTMLElement;
}

/** Whether the engine has a top layer for popovers. Without (a test, an old
 *  engine), a layer is a fixed element with a z-index. */
const TOP_LAYER = typeof HTMLElement !== "undefined" && "showPopover" in HTMLElement.prototype;
const POPOVER = TOP_LAYER ? "manual" : undefined;

/** Into the top layer, when the engine has one. */
function lift(el: HTMLElement): void {
  if (TOP_LAYER && !el.matches(":popover-open")) el.showPopover();
}

const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(v, hi));

export type Side = "bottom" | "top" | "right" | "left";

/**
 * A fixed layer next to `anchor`. Flips when there is no room, stays inside
 * the window. No dismiss logic: the owner closes it.
 */
export function Floating({
  anchor,
  open,
  side = "bottom",
  align = "start",
  gap = 4,
  className,
  children,
  ...rest
}: {
  anchor: HTMLElement | null;
  open: boolean;
  side?: Side;
  align?: "start" | "center" | "end";
  gap?: number;
  className?: string;
  children: React.ReactNode;
} & Omit<React.HTMLAttributes<HTMLDivElement>, "children" | "className">) {
  const ref = React.useRef<HTMLDivElement>(null);
  React.useLayoutEffect(() => {
    const el = ref.current;
    if (!open || !el || !anchor) return;
    lift(el);
    const r = anchor.getBoundingClientRect();
    const p = el.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    let top: number;
    let left: number;
    if (side === "right" || side === "left") {
      const fitsRight = r.right + gap + p.width <= vw - MARGIN;
      const fitsLeft = r.left - gap - p.width >= MARGIN;
      left = (side === "right" ? fitsRight || !fitsLeft : !fitsLeft && fitsRight) ? r.right + gap : r.left - gap - p.width;
      top = clamp(align === "center" ? r.top + r.height / 2 - p.height / 2 : r.top - 3, MARGIN, vh - p.height - MARGIN);
    } else {
      const below = vh - r.bottom - gap - MARGIN;
      const above = r.top - gap - MARGIN;
      const up = side === "top" ? p.height <= above || above > below : p.height > below && above > below;
      top = up ? r.top - gap - p.height : r.bottom + gap;
      left = align === "center" ? r.left + r.width / 2 - p.width / 2 : align === "end" ? r.right - p.width : r.left;
      left = clamp(left, MARGIN, vw - p.width - MARGIN);
      el.dataset.side = up ? "top" : "bottom";
    }
    el.style.top = `${toCss(top)}px`;
    el.style.left = `${toCss(left)}px`;
    el.style.visibility = "visible";
  });
  if (!open || !anchor) return null;
  return createPortal(
    <div ref={ref} popover={POPOVER} className={className} {...rest}>
      {children}
    </div>,
    hostOf(anchor),
  );
}

/**
 * A panel under its anchor (a dropdown, a menu, a popover). Flips up when
 * there is no room below, stays inside the window. Closes on Escape (focus
 * back on the anchor), on a pointer-down outside, on resize and on a scroll
 * that moves the anchor.
 */
export function AnchoredPanel({
  anchor,
  open,
  onClose,
  align = "start",
  matchWidth = true,
  className,
  children,
  ...aria
}: {
  anchor: React.RefObject<HTMLElement | null>;
  open: boolean;
  onClose: () => void;
  align?: "start" | "end";
  /** At least as wide as the anchor. */
  matchWidth?: boolean;
  className?: string;
  role?: string;
  id?: string;
  "aria-label"?: string;
  children: React.ReactNode;
}) {
  const ref = React.useRef<HTMLDivElement>(null);
  const close = React.useRef(onClose);
  React.useLayoutEffect(() => {
    close.current = onClose;
  });

  // Placed on every render while open: the content changes the height.
  React.useLayoutEffect(() => {
    const el = ref.current;
    const a = anchor.current;
    if (!open || !el || !a) return;
    lift(el);
    const r = a.getBoundingClientRect();
    el.style.maxHeight = "";
    el.style.minWidth = matchWidth ? `${toCss(r.width)}px` : "";
    const p = el.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    const below = vh - r.bottom - 2 - MARGIN;
    const above = r.top - 2 - MARGIN;
    const up = p.height > below && above > below;
    const room = up ? above : below;
    const h = Math.min(p.height, room);
    const top = up ? r.top - 2 - h : r.bottom + 2;
    const left = clamp(align === "end" ? r.right - p.width : r.left, MARGIN, vw - p.width - MARGIN);
    el.style.top = `${toCss(top)}px`;
    el.style.left = `${toCss(left)}px`;
    if (p.height > room) el.style.maxHeight = `${toCss(room)}px`;
    el.dataset.side = up ? "top" : "bottom";
    el.style.visibility = "visible";
  });

  React.useEffect(() => {
    if (!open) return;
    // A pointer-down in another open panel, or in a menu, is one nested in this one.
    const inside = (t: EventTarget | null) =>
      t instanceof Element &&
      (!!ref.current?.contains(t) ||
        !!anchor.current?.contains(t) ||
        !!t.closest('.d-pop, [data-slot="dropdown-menu-content"]'));
    const topmost = () => {
      const all = document.querySelectorAll(".d-pop");
      return all[all.length - 1] === ref.current;
    };
    const onDown = (e: PointerEvent) => {
      if (!inside(e.target)) close.current();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || !topmost()) return;
      e.preventDefault();
      e.stopPropagation();
      close.current();
      (anchor.current?.matches(FOCUSABLE) ? anchor.current : anchor.current?.querySelector<HTMLElement>(FOCUSABLE))?.focus({
        preventScroll: true,
      });
    };
    // Only a scroll that moves the anchor: a box beside it (a list that
    // shrinks under a choice made in the panel) leaves the panel in place.
    const onScroll = (e: Event) => {
      const t = e.target;
      if (t instanceof Node && ref.current?.contains(t)) return;
      if (t instanceof Element && anchor.current && !t.contains(anchor.current)) return;
      close.current();
    };
    const onResize = () => close.current();
    document.addEventListener("pointerdown", onDown, true);
    document.addEventListener("keydown", onKey, true);
    document.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", onResize);
    return () => {
      document.removeEventListener("pointerdown", onDown, true);
      document.removeEventListener("keydown", onKey, true);
      document.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", onResize);
    };
  }, [open, anchor]);

  if (!open || !anchor.current) return null;
  return createPortal(
    <div ref={ref} popover={POPOVER} className={["d-pop", className].filter(Boolean).join(" ")} {...aria}>
      {children}
    </div>,
    hostOf(anchor.current),
  );
}
