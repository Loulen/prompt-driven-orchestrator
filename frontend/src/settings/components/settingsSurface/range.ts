/** Whole numbers between `min` and `max`: the range when `value` is not one, else nothing. */
export function rangeError(value: string, min: number, max?: number): string | null {
  const n = Number(value.trim());
  const ok = value.trim() !== "" && Number.isInteger(n) && n >= min && (max === undefined || n <= max);
  if (ok) return null;
  return max === undefined ? `≥ ${min}` : `${min}–${max}`;
}
