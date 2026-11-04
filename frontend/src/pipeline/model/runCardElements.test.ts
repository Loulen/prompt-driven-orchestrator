import { describe, expect, it } from "vitest";
import { DEFAULT_APPEARANCE, isVisible, withElement, type Appearance } from "../../lib/appearance";
import { COMPACT_CARD } from "./compactCanvas";
import { RUN_CARD, runCardHeight, runCardSize } from "./runCardElements";

const shown = (appearance: Appearance) => (id: string) => isVisible(appearance, id);

describe("runCardSize", () => {
  it("packs the cells two per row: four cells are the detailed card, two one row, none the header alone", () => {
    expect(runCardHeight(4)).toBe(COMPACT_CARD.detailed);
    expect(runCardHeight(3)).toBe(COMPACT_CARD.detailed);
    expect(runCardHeight(2)).toBe(49);
    expect(runCardHeight(1)).toBe(49);
    expect(runCardHeight(0)).toBe(26);
  });

  it("draws the default card on one row: Elapsed time and Cost", () => {
    expect(runCardSize(shown(DEFAULT_APPEARANCE), false)).toEqual({ width: 208, height: 49 });
  });

  it("is the detailed card, drawn as stored, while every cell shows", () => {
    let appearance = withElement(DEFAULT_APPEARANCE, RUN_CARD.agent, true);
    appearance = withElement(appearance, RUN_CARD.iteration, true);
    expect(runCardSize(shown(appearance), false)).toBeNull();
  });

  it("shrinks to the header once every cell is hidden; the markers do not count", () => {
    let appearance = withElement(DEFAULT_APPEARANCE, RUN_CARD.elapsed, false);
    appearance = withElement(appearance, RUN_CARD.cost, false);
    appearance = withElement(appearance, RUN_CARD.loop, false);
    expect(runCardSize(shown(appearance), false)).toEqual({ width: 208, height: 26 });
  });

  it("gives « Compact nodes » the last word: one line, whatever the cells", () => {
    let appearance = withElement(DEFAULT_APPEARANCE, RUN_CARD.agent, true);
    appearance = withElement(appearance, RUN_CARD.iteration, true);
    expect(runCardSize(shown(appearance), true)).toEqual({ width: 168, height: 30 });
  });
});
