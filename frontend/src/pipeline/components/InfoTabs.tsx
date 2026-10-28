import { useLayoutEffect, useRef, useState, type CSSProperties, type KeyboardEvent } from "react";
import { ChevronDown } from "lucide-react";
import { IconButton, Menu } from "../../shared/ui/kit";
import type { Icon } from "../../shared/ui/kit";

export interface InfoTabItem {
  key: string;
  label: string;
  icon: Icon;
  /** The tooltip, when the label is shortened. */
  title?: string;
  /** A small dot in this color: something to come and look at. */
  dot?: string;
  dotTestId?: string;
  /** A count in place of the dot. */
  count?: number;
  countTitle?: string;
  countTestId?: string;
}

/** Room kept at the end of the strip for the overflow chevron. */
const MORE = 28;

/**
 * How many tabs fit in the strip: each tab is measured while shown, the live
 * one keeps its place among the shown ones. A strip with no width (not laid
 * out yet) shows them all.
 */
function useFit(count: number, active: number) {
  const ref = useRef<HTMLDivElement>(null);
  const widths = useRef<number[]>([]);
  const [fit, setFit] = useState(count);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const measure = () => {
      const avail = el.getBoundingClientRect().width;
      if (avail === 0) {
        setFit(count);
        return;
      }
      const tabs = [...el.querySelectorAll<HTMLElement>("[data-info-tab]")];
      const sizes = tabs.map((t, i) => {
        const w = t.getBoundingClientRect().width;
        if (w) widths.current[i] = w;
        return widths.current[i] ?? 80;
      });
      let used = 0;
      let n = 0;
      for (const w of sizes) {
        const room = n === sizes.length - 1 ? avail : avail - MORE;
        if (used + w > room) break;
        used += w;
        n += 1;
      }
      setFit(Math.max(1, n));
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [count]);
  // The shown tabs first; a live tab past them takes the last shown slot.
  const order = [...Array(count).keys()];
  const shown = Math.min(fit, count);
  if (active >= shown && active >= 0) {
    order.splice(active, 1);
    order.splice(shown - 1, 0, active);
  }
  return { ref, visible: order.slice(0, shown), hidden: order.slice(shown) };
}

/**
 * The tabs of Pipeline info: icon and label, underlined in ruby when live;
 * those that do not fit go behind a chevron at the end of the strip.
 * ArrowLeft / ArrowRight move and select.
 */
export default function InfoTabs({
  items,
  value,
  onChange,
}: {
  items: InfoTabItem[];
  value: string;
  onChange: (key: string) => void;
}) {
  const activeIndex = items.findIndex((t) => t.key === value);
  const { ref, visible, hidden } = useFit(items.length, activeIndex);
  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    const dir = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
    if (!dir || visible.length === 0) return;
    e.preventDefault();
    const at = visible.indexOf(activeIndex);
    const next = visible[(at + dir + visible.length) % visible.length];
    onChange(items[next].key);
    e.currentTarget.querySelector<HTMLElement>(`[data-testid="info-tab-${items[next].key}"]`)?.focus();
  };
  return (
    <div className="pi-tabs" ref={ref} role="tablist" aria-label="Pipeline info" onKeyDown={onKey} data-testid="pipeline-info-tabs">
      {items.map((t, i) => {
        const on = t.key === value;
        const slot = visible.indexOf(i);
        return (
          <button
            key={t.key}
            type="button"
            role="tab"
            data-info-tab
            aria-selected={on}
            tabIndex={on ? 0 : -1}
            hidden={slot < 0}
            title={t.title ?? t.label}
            className="pi-tab"
            style={{ order: slot }}
            onClick={() => onChange(t.key)}
            data-testid={`info-tab-${t.key}`}
          >
            <t.icon size={13} strokeWidth={1.5} aria-hidden />
            {t.label}
            {t.count != null ? (
              <span className="pi-tab__n" title={t.countTitle} data-testid={t.countTestId}>
                {t.count}
              </span>
            ) : (
              t.dot && (
                <span
                  className="pi-tab__dot"
                  style={{ ["--st" as string]: t.dot } as CSSProperties}
                  aria-hidden
                  data-testid={t.dotTestId}
                />
              )
            )}
          </button>
        );
      })}
      {hidden.length > 0 && (
        <span className="pi-tabs__more" style={{ order: items.length }}>
          <Menu
            label="More tabs"
            align="end"
            trigger={({ open, toggle }) => (
              <IconButton
                icon={ChevronDown}
                label={hidden.map((i) => items[i].label).join(", ")}
                pressed={open}
                onClick={toggle}
                data-testid="info-tabs-more"
              />
            )}
            entries={hidden.map((i) => ({
              label: items[i].title ?? items[i].label,
              icon: items[i].icon,
              hint: items[i].count != null ? String(items[i].count) : undefined,
              onSelect: () => onChange(items[i].key),
            }))}
          />
        </span>
      )}
    </div>
  );
}
