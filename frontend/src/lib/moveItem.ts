/**
 * `list` with the item at `from` moved to `to` (clamped into the list), the
 * others closing up: a tab dragged along the tab bar. The same list when
 * nothing moves.
 */
export function moveItem<T>(list: readonly T[], from: number, to: number): T[] {
  if (from < 0 || from >= list.length) return list as T[];
  const target = Math.max(0, Math.min(to, list.length - 1));
  if (target === from) return list as T[];
  const next = list.slice();
  const [item] = next.splice(from, 1);
  next.splice(target, 0, item);
  return next;
}
