/**
 * A Settings search hides what does not match (CORE-232): the rows filter
 * themselves, and the CSS hides the rest of a filtered section. A filtered
 * Appearance block renders each switch row inside its pointing wrapper
 * (`.set-ablock__point`): hiding every non-row child hid every matching switch
 * (« iteration », « cost »…) behind a bare section title. The cascade itself
 * is played in a browser by `e2e/settings-surface.spec.ts`.
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const CSS = readFileSync(join(import.meta.dirname, "settings.css"), "utf8");

describe("Settings search, filtered sections", () => {
  it("keeps the switch of an Appearance block that matches: its wrapper is not hidden", () => {
    expect(CSS).toContain('.set-sec__body[data-filtered="true"] > :not(.set-row, .set-ablock__point)');
    expect(CSS).toContain('.set-sec__body[data-filtered="true"] > .set-ablock__point:not(:has(.set-row))');
    expect(CSS).not.toMatch(/data-filtered="true"\] > :not\(\.set-row\),/);
  });
});
