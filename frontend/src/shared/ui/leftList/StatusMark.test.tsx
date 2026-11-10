/**
 * The Contrôle de sélection d'une liste (CONTEXT.md): round, as
 * `SelectControl` draws it. A ring of the status's colour on the row's hover
 * and while some row is ticked, a full round of the accent once ticked.
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import StatusMark from "./StatusMark";

const CSS = readFileSync(join(import.meta.dirname, "..", "..", "..", "app", "leftList.css"), "utf8");

/** The declarations of the first rule whose selector list is exactly `selector`. */
function rule(selector: string): string {
  const start = CSS.indexOf(`${selector} {`);
  if (start < 0) throw new Error(`rule not found: ${selector}`);
  return CSS.slice(start, CSS.indexOf("}", start));
}

describe("StatusMark", () => {
  it("carries the status's colour for its ring, neutral without a status", () => {
    const { rerender } = render(<StatusMark glyph={<span />} tone="failed" selected={false} label="Select a" onSelect={() => {}} testId="m" />);
    expect(screen.getByTestId("m")).toHaveAttribute("data-tone", "failed");
    expect(screen.getByTestId("m").style.getPropertyValue("--mark-tone")).toBe("var(--color-st-failed)");
    rerender(<StatusMark glyph={<span />} selected={false} label="Select a" onSelect={() => {}} testId="m" />);
    expect(screen.getByTestId("m").style.getPropertyValue("--mark-tone")).toBe("var(--text-3)");
  });

  it("is a checkbox: a click or Space selects, ticked it shows its check", () => {
    const onSelect = vi.fn();
    const { rerender } = render(<StatusMark glyph={<span />} tone="done" selected={false} label="Select a" onSelect={onSelect} />);
    const mark = screen.getByRole("checkbox", { name: "Select a" });
    expect(screen.queryByTestId("select-check")).toBeNull();
    fireEvent.click(mark);
    fireEvent.keyDown(mark, { key: " " });
    expect(onSelect).toHaveBeenCalledTimes(2);
    rerender(<StatusMark glyph={<span />} tone="done" selected label="Deselect a" onSelect={onSelect} />);
    expect(screen.getByRole("checkbox", { name: "Deselect a" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByTestId("select-check")).toBeInTheDocument();
  });

  it("draws a round 14 px ring of the status colour, shown on the row's hover, and a 16 px accent round once ticked", () => {
    const box = rule("  .lp-mark__box");
    expect(box).toMatch(/border-radius: 50%/);
    expect(box).toMatch(/width: 14px/);
    expect(box).toMatch(/border: 2px solid var\(--mark-tone/);
    // The whole row is the hover target, not the icon alone.
    expect(CSS).toMatch(/\.lp-row:hover \.lp-mark \.lp-mark__box \{\s*display: grid;/);
    const ticked = rule('  .lp-mark[aria-checked="true"] .lp-mark__box');
    expect(ticked).toMatch(/width: 16px/);
    expect(ticked).toMatch(/background: var\(--primary\)/);
    // The select-all of a group header is round too.
    expect(rule("  .lp-box")).toMatch(/border-radius: 50%/);
  });
});
