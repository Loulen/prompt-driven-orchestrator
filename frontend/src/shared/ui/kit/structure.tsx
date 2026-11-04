/**
 * The structure and display primitives of the shared design system
 * (ADR-0091): tabs, sections, chips, empty states, banners and
 * property grids. Styles in kit.css, tokens only.
 */
import * as React from "react";
import { ChevronDown, ChevronRight, X } from "lucide-react";
import { Tooltip } from "./tip";
import { ToneIcon } from "./tone";
import { toneColor, type Icon, type StatusTone, type Tone } from "./tones";
import { cx } from "./util";

// ================================================================== tabs

export interface TabItem<T extends string = string> {
  value: T;
  label: string;
  icon?: Icon;
  count?: number;
  kbd?: string;
  disabled?: boolean;
}

export interface TabsProps<T extends string = string> {
  items: TabItem<T>[];
  value: T;
  onChange: (value: T) => void;
  /** Accessible name of the strip. */
  label: string;
}

/**
 * A tab strip at the full height of its 32 px bar, the live tab underlined in
 * ruby. ArrowLeft / ArrowRight, Home / End move and select (one tab stop).
 */
export function Tabs<T extends string>({ items, value, onChange, label }: TabsProps<T>) {
  const onKey = (e: React.KeyboardEvent<HTMLDivElement>) => {
    const on = items.filter((t) => !t.disabled);
    const i = on.findIndex((t) => t.value === value);
    const next =
      e.key === "ArrowRight"
        ? on[(i + 1) % on.length]
        : e.key === "ArrowLeft"
          ? on[(i - 1 + on.length) % on.length]
          : e.key === "Home"
            ? on[0]
            : e.key === "End"
              ? on[on.length - 1]
              : null;
    if (!next) return;
    e.preventDefault();
    onChange(next.value);
    e.currentTarget.querySelectorAll<HTMLElement>('[role="tab"]')[items.indexOf(next)]?.focus();
  };
  return (
    <div className="d-tabs" role="tablist" aria-label={label} onKeyDown={onKey}>
      {items.map((t) => {
        const on = t.value === value;
        const tab = (
          <button
            key={t.value}
            type="button"
            role="tab"
            aria-selected={on}
            tabIndex={on ? 0 : -1}
            disabled={t.disabled}
            className="d-tab"
            onClick={() => onChange(t.value)}
          >
            {t.icon && <t.icon size={13} strokeWidth={1.5} aria-hidden />}
            <span>{t.label}</span>
            {t.count != null && <span className="d-tab__n">{t.count}</span>}
          </button>
        );
        return t.kbd ? (
          <Tooltip key={t.value} label={t.label} kbd={t.kbd}>
            {tab}
          </Tooltip>
        ) : (
          tab
        );
      })}
    </div>
  );
}

// ================================================================== section

export interface SectionProps {
  title: string;
  count?: number;
  /** Icon buttons at the right of the head, shown on hover. */
  aside?: React.ReactNode;
  /** Starts folded (the head only). */
  closed?: boolean;
  /** A body without side padding (lists, tables). */
  flush?: boolean;
  children?: React.ReactNode;
}

/** A section: 24 px caps head with a chevron and a count; folds. */
export function Section({ title, count, aside, closed, flush, children }: SectionProps) {
  const [open, setOpen] = React.useState(!closed);
  const Chevron = open ? ChevronDown : ChevronRight;
  return (
    <section className="d-sec">
      <header className="d-sec__head">
        <button type="button" className="d-sec__toggle" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
          <Chevron size={14} strokeWidth={1.5} aria-hidden />
          <span className="d-caps d-sec__title">{title}</span>
          {count != null && <span className="d-chip d-sec__count">{count}</span>}
        </button>
        {aside && <span className="d-sec__aside">{aside}</span>}
      </header>
      {open && <div className={cx("d-sec__body", flush && "is-flush")}>{children}</div>}
    </section>
  );
}

// ================================================================== chip

export interface ChipProps {
  children: React.ReactNode;
  /** A status: its icon in its colour, the text stays muted. */
  tone?: Tone;
  icon?: Icon;
  mono?: boolean;
  title?: string;
  /** A removable chip (a set filter). */
  onRemove?: () => void;
  removeLabel?: string;
}

/** A neutral chip, 18 px: a fill, the tone as its status icon. */
export function Chip({ children, tone = "neutral", icon: Icon, mono, title, onRemove, removeLabel = "Remove" }: ChipProps) {
  return (
    <span className={cx("d-chip", mono && "d-chip--mono")} title={title}>
      {Icon ? <Icon size={11} strokeWidth={1.75} aria-hidden /> : tone !== "neutral" && <ToneIcon tone={tone} size={11} />}
      <span className="d-chip__t">{children}</span>
      {onRemove && (
        <button type="button" className="d-chip__x" aria-label={removeLabel} title={removeLabel} onClick={onRemove}>
          <X size={10} strokeWidth={2} aria-hidden />
        </button>
      )}
    </span>
  );
}

// ================================================================== empty state, banner, key/value

export interface EmptyStateProps {
  icon: Icon;
  /** 2 to 4 words. */
  children: React.ReactNode;
  action?: React.ReactNode;
  /** center: a pane or a list (icon above). row: one line inside a section. */
  layout?: "center" | "row";
  /** The icon spins (loading). */
  spin?: boolean;
}

/** An icon, 2 to 4 words, one action. Loading: the spinner and one word; error: a failed icon and the retry. */
export function EmptyState({ icon: Icon, children, action, layout = "center", spin }: EmptyStateProps) {
  const row = layout === "row";
  return (
    <div className={cx("d-empty", row && "d-empty--row")} role="status">
      <Icon size={row ? 14 : 20} strokeWidth={row ? 1.5 : 1.25} aria-hidden className={spin ? "d-spin" : undefined} />
      <span className="d-empty__t">{children}</span>
      {action}
    </div>
  );
}

export interface BannerProps {
  tone: StatusTone;
  title: string;
  /** One line; the full text in the tooltip. */
  text?: string;
  actions?: React.ReactNode;
  /** Drawn instead of the tone's status icon, in the tone's colour (a lock on a read-only state). */
  icon?: Icon;
}

/** A tinted line at the top of a pane: the status icon (or `icon`), a short fact, the detail, actions. */
export function Banner({ tone, title, text, actions, icon: BannerIcon }: BannerProps) {
  return (
    <div
      className="d-banner"
      style={{ ["--st" as string]: toneColor(tone) }}
      role={tone === "failed" ? "alert" : "status"}
      title={text}
    >
      {BannerIcon ? (
        <BannerIcon size={14} strokeWidth={1.5} className="d-banner__icon" aria-hidden />
      ) : (
        <ToneIcon tone={tone} size={14} />
      )}
      <strong className="d-banner__title">{title}</strong>
      {text && <span className="d-banner__text">{text}</span>}
      {actions && <span className="d-banner__actions">{actions}</span>}
    </div>
  );
}

export interface KeyValueRow {
  k: string;
  v: React.ReactNode;
  mono?: boolean;
  /** The tooltip of the value, when it is not plain text. */
  tip?: string;
  /** `data-*` attributes of the row (a preview's marks). */
  attrs?: Readonly<Record<`data-${string}`, string | true | undefined>>;
}

/** A read-only property grid: a 96 px key column, 24 px rows, ellipsis. */
export function KeyValue({ rows }: { rows: KeyValueRow[] }) {
  return (
    <dl className="d-kv">
      {rows.map((r) => (
        <div key={r.k} className="d-kv__row" {...r.attrs}>
          <dt title={r.k}>{r.k}</dt>
          <dd className={r.mono ? "d-mono" : undefined} title={r.tip ?? (typeof r.v === "string" ? r.v : undefined)}>
            {r.v}
          </dd>
        </div>
      ))}
    </dl>
  );
}
