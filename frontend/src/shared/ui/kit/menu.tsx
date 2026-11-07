/**
 * Popover and Menu of the shared design system: a trigger and a floating panel
 * under it.
 */
import * as React from "react";
import { Check, ChevronRight, Lock } from "lucide-react";
import { AnchoredPanel, FOCUSABLE, Floating } from "./layer";
import { cx } from "./util";
import type { Icon } from "./tones";

// ================================================================== popover

export interface PopoverProps {
  /** The trigger, given whether the panel is open and how to toggle it. */
  trigger: (p: { open: boolean; toggle: () => void }) => React.ReactNode;
  /** The panel, or a function given how to close it. */
  children: React.ReactNode | ((close: () => void) => React.ReactNode);
  /** Accessible name of the panel. */
  label: string;
  align?: "start" | "end";
  className?: string;
}

/**
 * A non-modal panel under its trigger. Opening focuses the first control in
 * it; Escape, a click outside, a scroll or a resize close it; Escape returns
 * the focus to the trigger.
 */
export function Popover({ trigger, children, label, align = "start", className }: PopoverProps) {
  const [open, setOpen] = React.useState(false);
  const anchor = React.useRef<HTMLSpanElement>(null);
  const id = React.useId();
  const close = React.useCallback(() => {
    if (document.getElementById(id)?.contains(document.activeElement)) {
      document.getElementById(`${id}-anchor`)?.querySelector<HTMLElement>(FOCUSABLE)?.focus({ preventScroll: true });
    }
    setOpen(false);
  }, [id]);
  React.useEffect(() => {
    if (!open) return;
    const t = requestAnimationFrame(() =>
      document.getElementById(id)?.querySelector<HTMLElement>(FOCUSABLE)?.focus({ preventScroll: true }),
    );
    return () => cancelAnimationFrame(t);
  }, [open, id]);
  return (
    <span className="d-anchor" ref={anchor} id={`${id}-anchor`}>
      {trigger({ open, toggle: () => (open ? close() : setOpen(true)) })}
      <AnchoredPanel
        anchor={anchor}
        open={open}
        onClose={close}
        align={align}
        matchWidth={false}
        role="dialog"
        aria-label={label}
        id={id}
        className={cx("d-pop--panel", className)}
      >
        {typeof children === "function" ? children(close) : children}
      </AnchoredPanel>
    </span>
  );
}

// ================================================================== menu

export interface MenuItem {
  kind?: "item";
  label: string;
  icon?: Icon;
  kbd?: string;
  /** Right-aligned mono meta (a count, a value). */
  hint?: string;
  /** A check item (`menuitemcheckbox`), or a radio item with `radio`. */
  checked?: boolean;
  radio?: boolean;
  danger?: boolean;
  disabled?: boolean;
  /**
   * Why the caller may not pick it: the entry stays, greyed, and the menu
   * says this sentence once, on a line at its foot (with a lock), rather
   * than a tooltip per entry. Implies `disabled`.
   */
  disabledReason?: string;
  onSelect?: () => void;
  submenu?: MenuEntry[];
}

export type MenuEntry = MenuItem | { kind: "separator" } | { kind: "label"; label: string };

const isItem = (e: MenuEntry): e is MenuItem => !e.kind || e.kind === "item";
const isOff = (it: MenuItem) => !!it.disabled || !!it.disabledReason;

/** The reasons of the greyed entries of `entries`, each once, in order. */
function menuReasons(entries: readonly MenuEntry[]): string[] {
  const reasons: string[] = [];
  for (const e of entries) {
    if (isItem(e) && e.disabledReason && !reasons.includes(e.disabledReason)) reasons.push(e.disabledReason);
  }
  return reasons;
}

/**
 * The foot of a menu: why its greyed entries are greyed, a line per reason.
 * Inside an element of role `menu`, give it `id` and point the menu's
 * `aria-describedby` at it: it is then `presentation`, read as the menu's
 * description.
 */
export function MenuReasons({ reasons, id }: { reasons: readonly string[]; id?: string }) {
  if (reasons.length === 0) return null;
  const lines = reasons.map((reason) => (
    <div key={reason} className="d-pop__why" role={id ? undefined : "note"} data-testid="menu-disabled-reason">
      <Lock size={12} strokeWidth={1.5} aria-hidden />
      <span>{reason}</span>
    </div>
  ));
  return id ? (
    <div id={id} role="presentation">
      {lines}
    </div>
  ) : (
    <>{lines}</>
  );
}
const roleOf = (it: MenuItem) => (it.radio ? "menuitemradio" : it.checked !== undefined ? "menuitemcheckbox" : "menuitem");

/** The rows of a menu: a check column, the icon, the label, the shortcut. */
function MenuList({
  entries,
  label,
  onPick,
  openSub,
  onSubOpen,
}: {
  entries: MenuEntry[];
  label: string;
  onPick: (item: MenuItem) => void;
  openSub: number | null;
  onSubOpen: (index: number | null, row: HTMLElement | null) => void;
}) {
  return (
    <>
    <div role="menu" aria-label={label} style={{ display: "flex", flexDirection: "column", minWidth: 0 }}>
      {entries.map((e, i) => {
        if (e.kind === "separator") return <div key={i} className="d-pop__sep" role="separator" />;
        if (e.kind === "label")
          return (
            <div key={i} className="d-caps d-pop__group" role="presentation">
              {e.label}
            </div>
          );
        const it = e as MenuItem;
        const Ic = it.icon;
        const subOpen = openSub === i;
        return (
          <button
            key={i}
            type="button"
            role={roleOf(it)}
            tabIndex={-1}
            data-menuitem=""
            aria-checked={it.checked !== undefined ? it.checked : undefined}
            aria-disabled={isOff(it) || undefined}
            aria-haspopup={it.submenu ? "menu" : undefined}
            aria-expanded={it.submenu ? subOpen : undefined}
            className={cx("d-pop__row", it.danger && "is-danger")}
            data-hi={subOpen || undefined}
            onPointerEnter={(ev) => onSubOpen(it.submenu ? i : null, it.submenu ? ev.currentTarget : null)}
            onClick={(ev) => {
              if (isOff(it)) return;
              if (it.submenu) onSubOpen(subOpen ? null : i, ev.currentTarget);
              else onPick(it);
            }}
          >
            <span className="d-pop__mark">
              {it.checked ? it.radio ? <span className="d-pop__dot" /> : <Check size={12} strokeWidth={2} aria-hidden /> : null}
            </span>
            {Ic && <Ic className="d-pop__ic" size={14} strokeWidth={1.5} aria-hidden />}
            <span className="d-pop__label">{it.label}</span>
            {it.hint && <span className="d-pop__hint">{it.hint}</span>}
            {it.kbd && <span className="d-kbd d-pop__kbd">{it.kbd}</span>}
            {it.submenu && <ChevronRight className="d-pop__sub" size={12} strokeWidth={1.5} aria-hidden />}
          </button>
        );
      })}
    </div>
    <MenuReasons reasons={menuReasons(entries)} />
    </>
  );
}

const menuRows = (root: Element | null) => [
  ...(root?.querySelectorAll<HTMLElement>("[data-menuitem]:not([aria-disabled])") ?? []),
];

function moveFocus(root: Element | null, dir: 1 | -1 | "first" | "last" | string) {
  const rows = menuRows(root);
  if (!rows.length) return;
  const i = rows.indexOf(document.activeElement as HTMLElement);
  let next: HTMLElement | undefined;
  if (dir === "first") next = rows[0];
  else if (dir === "last") next = rows[rows.length - 1];
  else if (dir === 1 || dir === -1) next = rows[(i + dir + rows.length) % rows.length];
  else {
    const ch = String(dir).toLowerCase();
    next = [...rows.slice(i + 1), ...rows.slice(0, i + 1)].find((r) => r.textContent?.trim().toLowerCase().startsWith(ch));
  }
  next?.focus({ preventScroll: true });
}

/** Arrow keys, Home / End and type-ahead in one menu; ArrowRight opens the row's submenu. */
function menuKeys(e: React.KeyboardEvent, root: HTMLElement | null) {
  if (!(e.target instanceof Element) || !root?.contains(e.target)) return;
  switch (e.key) {
    case "ArrowDown":
      e.preventDefault();
      moveFocus(root, 1);
      break;
    case "ArrowUp":
      e.preventDefault();
      moveFocus(root, -1);
      break;
    case "Home":
      e.preventDefault();
      moveFocus(root, "first");
      break;
    case "End":
      e.preventDefault();
      moveFocus(root, "last");
      break;
    case "ArrowRight": {
      const r = e.target as HTMLElement;
      if (r.getAttribute("aria-haspopup") !== "menu") break;
      e.preventDefault();
      if (r.getAttribute("aria-expanded") !== "true") r.click();
      requestAnimationFrame(() => {
        const subs = document.querySelectorAll(".d-pop--sub");
        moveFocus(subs[subs.length - 1] ?? null, "first");
      });
      break;
    }
    case "Tab":
      e.preventDefault();
      break;
    default:
      if (e.key.length === 1 && /\S/.test(e.key) && !e.ctrlKey && !e.metaKey && !e.altKey) moveFocus(root, e.key);
  }
}

/** A submenu, right of its row; ArrowLeft or Escape closes it and returns to the row. */
function SubMenu({
  row,
  entries,
  label,
  onPick,
  onClose,
}: {
  row: HTMLElement;
  entries: MenuEntry[];
  label: string;
  onPick: (it: MenuItem) => void;
  onClose: (refocus: boolean) => void;
}) {
  const ref = React.useRef<HTMLDivElement>(null);
  const [sub, setSub] = React.useState<{ i: number; row: HTMLElement } | null>(null);
  const item = sub ? entries[sub.i] : null;
  return (
    <Floating anchor={row} open side="right" gap={2} className="d-pop d-pop--sub">
      <div
        ref={ref}
        onKeyDown={(e) => {
          if (sub && e.key === "Escape") {
            e.preventDefault();
            e.stopPropagation();
            setSub(null);
            return;
          }
          if (e.key === "ArrowLeft" || e.key === "Escape") {
            e.preventDefault();
            e.stopPropagation();
            onClose(true);
            return;
          }
          menuKeys(e, ref.current);
        }}
      >
        <MenuList
          entries={entries}
          label={label}
          onPick={onPick}
          openSub={sub?.i ?? null}
          onSubOpen={(i, r) => setSub(i != null && r ? { i, row: r } : null)}
        />
      </div>
      {item && isItem(item) && item.submenu && sub && (
        <SubMenu
          row={sub.row}
          entries={item.submenu}
          label={item.label}
          onPick={onPick}
          onClose={(f) => {
            setSub(null);
            if (f) sub.row.focus();
          }}
        />
      )}
    </Floating>
  );
}

export interface MenuProps {
  /** The trigger, given whether the menu is open and how to toggle it. */
  trigger: (p: { open: boolean; toggle: () => void }) => React.ReactNode;
  entries: MenuEntry[];
  /** Accessible name of the menu. */
  label: string;
  align?: "start" | "end";
}

/**
 * A trigger and a menu under it. Opening focuses the first item; ArrowUp /
 * ArrowDown, Home / End and a letter move; Enter or Space picks; ArrowRight
 * opens a submenu, ArrowLeft closes it; Escape closes and returns to the
 * trigger.
 */
export function Menu({ trigger, entries, label, align = "start" }: MenuProps) {
  const [open, setOpen] = React.useState(false);
  const [sub, setSub] = React.useState<{ i: number; row: HTMLElement } | null>(null);
  const anchor = React.useRef<HTMLSpanElement>(null);
  const list = React.useRef<HTMLDivElement>(null);
  const focusTrigger = () => anchor.current?.querySelector<HTMLElement>(FOCUSABLE)?.focus({ preventScroll: true });
  const close = React.useCallback(() => {
    setOpen(false);
    setSub(null);
  }, []);
  const pick = (it: MenuItem) => {
    close();
    focusTrigger();
    it.onSelect?.();
  };
  React.useEffect(() => {
    if (!open) return;
    const t = requestAnimationFrame(() => moveFocus(list.current, "first"));
    return () => cancelAnimationFrame(t);
  }, [open]);
  const item = sub ? entries[sub.i] : null;
  return (
    <span className="d-anchor" ref={anchor}>
      {trigger({ open, toggle: () => (open ? close() : setOpen(true)) })}
      <AnchoredPanel
        anchor={anchor}
        open={open}
        onClose={() => {
          if (list.current?.contains(document.activeElement)) focusTrigger();
          close();
        }}
        align={align}
        matchWidth={false}
        className="d-pop--panel"
      >
        <div
          ref={list}
          onKeyDown={(e) => {
            if (e.key === "Escape" && sub) {
              e.preventDefault();
              e.stopPropagation();
              setSub(null);
              return;
            }
            menuKeys(e, list.current);
          }}
        >
          <MenuList
            entries={entries}
            label={label}
            onPick={pick}
            openSub={sub?.i ?? null}
            onSubOpen={(i, r) => setSub(i != null && r ? { i, row: r } : null)}
          />
        </div>
      </AnchoredPanel>
      {open && item && isItem(item) && item.submenu && sub && (
        <SubMenu
          row={sub.row}
          entries={item.submenu}
          label={item.label}
          onPick={pick}
          onClose={(f) => {
            setSub(null);
            if (f) sub.row.focus();
          }}
        />
      )}
    </span>
  );
}
