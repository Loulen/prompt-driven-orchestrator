/** Class names, the falsy ones dropped. */
export const cx = (...c: (string | false | null | undefined)[]): string => c.filter(Boolean).join(" ");

/** Tooltip text: the name, then the shortcut in brackets: "Retry (R)". */
export const tip = (label: string, kbd?: string): string => (kbd ? `${label} (${kbd})` : label);

/** "Alice Martin" → "AM", "bob" → "B". */
export function initials(name: string): string {
  const w = name.trim().split(/\s+/).filter(Boolean);
  const l = w.length > 1 ? [w[0], w[w.length - 1]] : w.slice(0, 1);
  return l.map((m) => [...m][0]?.toUpperCase() ?? "").join("") || "?";
}
