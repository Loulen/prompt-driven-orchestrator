/**
 * The rows of a list's preview in Settings › Appearance: the most recent item, then
 * the most recent one that shows an element `wanted` looks for when the first does
 * not, else the next most recent. So that element appears once in the preview.
 */
export function recentPair<T>(items: readonly T[], at: (item: T) => string | null | undefined, wanted: (item: T) => boolean): T[] {
  // The latest first; an item without a date after, in the list's order.
  const sorted = [...items].sort((a, b) => {
    const x = at(a) ?? "";
    const y = at(b) ?? "";
    return x === y ? 0 : x < y ? 1 : -1;
  });
  const [recent, ...others] = sorted;
  if (recent === undefined) return [];
  const second = (wanted(recent) ? undefined : others.find(wanted)) ?? others[0];
  return second === undefined ? [recent] : [recent, second];
}
