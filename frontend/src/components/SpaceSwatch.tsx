import type { Space } from "../types";
import { spaceColorVar } from "../lib/spacePalette";

/**
 * The mark of an Espace (#1041): a small SQUARE swatch in its colour. Square on
 * purpose — every Run status is a round dot, so a shape apart keeps the two
 * readable side by side whatever the palette (colour-blind one included).
 */
export default function SpaceSwatch({
  space,
  size = 8,
  testId = "space-swatch",
}: {
  space: Space;
  size?: number;
  testId?: string;
}) {
  return (
    <span
      role="img"
      aria-label={`Space: ${space.name}`}
      title={`Space: ${space.name}`}
      data-testid={testId}
      data-space-color={space.color}
      className="inline-block shrink-0 rounded-[2px]"
      style={{ width: size, height: size, background: spaceColorVar(space.color) }}
    />
  );
}
