import { ArrowDown, ArrowUp } from "lucide-react";
import type { TableSort } from "./tableSort";

/** A sortable column header: the label, and the arrow of the way once it sorts. */
export function SortTh({
  sort,
  onSort,
  sortKey,
  children,
  className,
  title,
  testid,
  after,
}: {
  sort: TableSort;
  onSort: (key: string) => void;
  sortKey: string;
  children: React.ReactNode;
  className?: string;
  title?: string;
  testid?: string;
  /** After the sort button, outside it (an « i » with its own tooltip). */
  after?: React.ReactNode;
}) {
  const on = sort.key === sortKey;
  const Icon = sort.dir === "asc" ? ArrowUp : ArrowDown;
  return (
    <th
      className={className}
      aria-sort={on ? (sort.dir === "asc" ? "ascending" : "descending") : undefined}
      data-testid={testid}
    >
      <button type="button" className="sx-th" data-on={on || undefined} title={title} onClick={() => onSort(sortKey)}>
        {children}
        {on && <Icon size={11} strokeWidth={1.75} aria-hidden />}
      </button>
      {after}
    </th>
  );
}

/** The share bar behind a figure: its width against the column's largest. */
export function InBar({ value, max, top }: { value: number | null | undefined; max: number; top?: boolean }) {
  if (!value || max <= 0) return null;
  return (
    <span
      className="sx-inbar"
      data-top={top || undefined}
      style={{ width: `${Math.min(100, (value / max) * 100)}%` }}
      aria-hidden
    />
  );
}
