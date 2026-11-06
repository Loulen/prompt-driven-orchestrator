/**
 * The fixed palette of an Espace (#1041, CONTEXT.md « Espace »).
 *
 * Eight colour KEYS, in the order a new Espace takes them — the same list, in the
 * same order, as `space_store::PALETTE` in the daemon, which stores the key and
 * picks the first free one. The values live in `index.css` as
 * `--color-space-<key>`, one per theme, so a theme switch recolours every swatch
 * with no re-render; `themePalette.test.ts` holds each at 3:1 on every surface of
 * both themes. They are Okabe–Ito hues (apart under red-green colour blindness)
 * and are only ever painted as a stripe or a SQUARE swatch, never as a round
 * dot, so they cannot be mistaken for a Run status.
 */

export const SPACE_COLORS = [
  "blue",
  "orange",
  "green",
  "pink",
  "sky",
  "vermillion",
  "violet",
  "yellow",
] as const;

export type SpaceColor = (typeof SPACE_COLORS)[number];

/** Human label of a colour key, for the picker's accessible name. */
export const SPACE_COLOR_LABELS: Record<SpaceColor, string> = {
  blue: "Blue",
  orange: "Orange",
  green: "Green",
  pink: "Pink",
  sky: "Sky blue",
  vermillion: "Vermillion",
  violet: "Violet",
  yellow: "Yellow",
};

export function isSpaceColor(value: string): value is SpaceColor {
  return (SPACE_COLORS as readonly string[]).includes(value);
}

/**
 * The CSS colour of a key: the theme token. An unknown key (a newer daemon's
 * palette) falls back to a neutral grey rather than painting nothing.
 */
export function spaceColorVar(color: string): string {
  return isSpaceColor(color) ? `var(--color-space-${color})` : "var(--color-fg-4)";
}

/**
 * The colour a new Espace takes: the first key no Espace uses yet; once all
 * eight are taken, the first among the least used. Mirror of the daemon's
 * `first_free_color`, so the create dialog pre-selects what the daemon would.
 */
export function firstFreeSpaceColor(used: readonly string[]): SpaceColor {
  const count = (c: string) => used.filter((u) => u === c).length;
  const min = Math.min(...SPACE_COLORS.map(count));
  return SPACE_COLORS.find((c) => count(c) === min) ?? SPACE_COLORS[0];
}
