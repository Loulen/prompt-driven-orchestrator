import { useState } from "react";

/** The sort of a breakdown table (`overlays.statsBreakdown`): a column, a way. */
export interface TableSort {
  key: string;
  dir: "asc" | "desc";
}

/** A table's sort, starting on `initial`: a click on a header sorts by it,
 *  names ascending and figures descending; a second click turns it round. */
export function useTableSort(initial: TableSort): [TableSort, (key: string) => void] {
  const [sort, setSort] = useState(initial);
  const toggle = (key: string) =>
    setSort((current) =>
      current.key === key
        ? { key, dir: current.dir === "asc" ? "desc" : "asc" }
        : { key, dir: key === "name" ? "asc" : "desc" },
    );
  return [sort, toggle];
}

/** The rows in the table's order; a missing figure always sorts last. */
export function sortRows<T>(
  rows: T[],
  sort: TableSort,
  value: (row: T, key: string) => number | string | null | undefined,
): T[] {
  const dir = sort.dir === "asc" ? 1 : -1;
  return [...rows].sort((a, b) => {
    const va = value(a, sort.key);
    const vb = value(b, sort.key);
    if (va == null || vb == null) return va == null ? (vb == null ? 0 : 1) : -1;
    if (typeof va === "string" || typeof vb === "string")
      return String(va).localeCompare(String(vb)) * dir;
    return (va - vb) * dir;
  });
}

