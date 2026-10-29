/**
 * The dropdown of the shared design system: one value out of a list, under a
 * bordered 24 px trigger.
 */
import * as React from "react";
import { Check, ChevronDown, Search } from "lucide-react";
import { AnchoredPanel } from "./layer";
import { cx } from "./util";
import type { Icon } from "./tones";

export interface SelectOption<V extends string = string> {
  value: V;
  label: string;
  icon?: Icon;
  /** Right-aligned meta in the row (a count, a path). */
  hint?: string;
  /** Rows of the same group are listed under a caps header, in the order given. */
  group?: string;
  disabled?: boolean;
  title?: string;
}

export interface SelectProps<V extends string = string> {
  value: V | null;
  onChange: (value: V) => void;
  options: SelectOption<V>[];
  /** Accessible name and tooltip. */
  label: string;
  /** Trigger text when no option has the value (default: the label). */
  placeholder?: string;
  /** Leading icon of the trigger (the current option's icon wins). */
  icon?: Icon;
  mono?: boolean;
  size?: "md" | "sm";
  disabled?: boolean;
  /** A search box over the list; by default when there are more than 8 options. */
  search?: boolean;
  align?: "start" | "end";
  className?: string;
}

const norm = (s: string) => s.toLowerCase();

/**
 * A single-value dropdown. ArrowDown / ArrowUp, Home / End and a letter move,
 * Enter picks, Escape closes; with a search box, typing filters.
 */
export function Select<V extends string>({
  value,
  onChange,
  options,
  label,
  placeholder,
  icon,
  mono,
  size = "md",
  disabled,
  search,
  align,
  className,
}: SelectProps<V>) {
  const [open, setOpen] = React.useState(false);
  const [q, setQ] = React.useState("");
  const [hi, setHi] = React.useState(-1);
  const btn = React.useRef<HTMLButtonElement>(null);
  const listRef = React.useRef<HTMLDivElement>(null);
  const typed = React.useRef({ s: "", t: 0 });
  const uid = React.useId();
  const withSearch = search ?? options.length > 8;
  const cur = options.find((o) => o.value === value);
  const needle = norm(q.trim());
  const shown = needle
    ? options.filter((o) => norm(o.label).includes(needle) || norm(o.hint ?? "").includes(needle))
    : options;

  const step = (from: number, dir: 1 | -1) => {
    for (let i = 1; i <= shown.length; i++) {
      const j = (from + dir * i + shown.length * 2) % shown.length;
      if (!shown[j].disabled) return j;
    }
    return -1;
  };
  const openList = () => {
    if (disabled) return;
    setQ("");
    const i = options.findIndex((o) => o.value === value);
    setHi(i >= 0 ? i : options.findIndex((o) => !o.disabled));
    setOpen(true);
  };
  const closeList = () => {
    setOpen(false);
    setQ("");
  };
  const pick = (o: SelectOption<V> | undefined) => {
    if (!o || o.disabled) return;
    if (o.value !== value) onChange(o.value);
    closeList();
    btn.current?.focus({ preventScroll: true });
  };

  React.useEffect(() => {
    if (!open || hi < 0) return;
    listRef.current?.querySelector<HTMLElement>(`[data-i="${hi}"]`)?.scrollIntoView?.({ block: "nearest" });
  }, [open, hi]);

  const typeAhead = (ch: string) => {
    const now = Date.now();
    typed.current = { s: now - typed.current.t > 600 ? ch : typed.current.s + ch, t: now };
    const s = norm(typed.current.s);
    const start = typed.current.s.length === 1 ? hi + 1 : Math.max(hi, 0);
    for (let k = 0; k < shown.length; k++) {
      const j = (start + k) % shown.length;
      if (!shown[j].disabled && norm(shown[j].label).startsWith(s)) {
        setHi(j);
        return;
      }
    }
  };

  const onKey = (e: React.KeyboardEvent, fromSearch: boolean) => {
    if (!open) {
      if (["ArrowDown", "ArrowUp", "Enter", " "].includes(e.key)) {
        e.preventDefault();
        openList();
      }
      return;
    }
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        setHi(step(hi, 1));
        break;
      case "ArrowUp":
        e.preventDefault();
        setHi(step(hi < 0 ? 0 : hi, -1));
        break;
      case "Home":
        if (!fromSearch) {
          e.preventDefault();
          setHi(step(-1, 1));
        }
        break;
      case "End":
        if (!fromSearch) {
          e.preventDefault();
          setHi(step(0, -1));
        }
        break;
      case "Enter":
        e.preventDefault();
        pick(shown[hi]);
        break;
      case " ":
        if (!fromSearch) {
          e.preventDefault();
          pick(shown[hi]);
        }
        break;
      case "Tab":
        closeList();
        break;
      default:
        if (!fromSearch && e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) typeAhead(e.key);
    }
  };

  const Lead = cur?.icon ?? icon;
  const optId = (i: number) => `${uid}-o${i}`;
  return (
    <>
      <button
        ref={btn}
        type="button"
        disabled={disabled}
        title={label}
        aria-label={label}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={open ? `${uid}-list` : undefined}
        aria-activedescendant={open && !withSearch && hi >= 0 ? optId(hi) : undefined}
        className={cx("d-sel", size === "sm" && "d-sel--sm", mono && "d-sel--mono", className)}
        onClick={() => (open ? closeList() : openList())}
        onKeyDown={(e) => onKey(e, false)}
      >
        {Lead && <Lead className="d-sel__lead" size={size === "sm" ? 12 : 13} strokeWidth={1.5} aria-hidden />}
        <span className={cx("d-sel__val", !cur && "d-sel__val--ph")}>{cur?.label ?? placeholder ?? label}</span>
        <ChevronDown className="d-sel__chev" size={size === "sm" ? 11 : 12} strokeWidth={1.5} aria-hidden />
      </button>
      <AnchoredPanel anchor={btn} open={open} onClose={closeList} align={align}>
        {withSearch && (
          <label className="d-pop__search">
            <Search size={13} strokeWidth={1.5} aria-hidden />
            <input
              autoFocus
              value={q}
              placeholder={label}
              aria-label={label}
              spellCheck={false}
              onChange={(e) => {
                setQ(e.target.value);
                setHi(0);
              }}
              onKeyDown={(e) => onKey(e, true)}
            />
          </label>
        )}
        <div ref={listRef} id={`${uid}-list`} role="listbox" aria-label={label} className="d-pop__list">
          {shown.map((o, i) => {
            const g = o.group ?? "";
            const pg = i > 0 ? (shown[i - 1].group ?? "") : null;
            const head =
              g === pg ? null : g ? (
                <div className="d-caps d-pop__group">{g}</div>
              ) : pg !== null ? (
                <div className="d-pop__sep" />
              ) : null;
            const Ic = o.icon;
            return (
              <React.Fragment key={o.value}>
                {head}
                <div
                  id={optId(i)}
                  data-i={i}
                  role="option"
                  aria-selected={o.value === value}
                  aria-disabled={o.disabled || undefined}
                  className="d-pop__row"
                  data-hi={i === hi || undefined}
                  title={o.title}
                  onPointerMove={() => {
                    if (!o.disabled && hi !== i) setHi(i);
                  }}
                  onMouseDown={(e) => e.preventDefault()}
                  onClick={() => pick(o)}
                >
                  <span className="d-pop__mark">{o.value === value && <Check size={12} strokeWidth={2} aria-hidden />}</span>
                  {Ic && <Ic className="d-pop__ic" size={13} strokeWidth={1.5} aria-hidden />}
                  <span className={cx("d-pop__label", mono && "d-pop__label--mono")}>{o.label}</span>
                  {o.hint && <span className="d-pop__hint">{o.hint}</span>}
                </div>
              </React.Fragment>
            );
          })}
          {shown.length === 0 && <div className="d-pop__empty">No match</div>}
        </div>
      </AnchoredPanel>
    </>
  );
}
