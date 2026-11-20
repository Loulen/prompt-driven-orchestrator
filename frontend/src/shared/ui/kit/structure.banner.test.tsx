/**
 * The kit's Banner: one line ending in an ellipsis by default; with `wrap`, a
 * whole sentence over several lines, at body weight, its actions on the right.
 */
import { render, screen } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { Banner, Button } from "./index";

const SENTENCE = "Editing as markdown replaces the blocks of this page when you save, comments included.";

const css = readFileSync(join(import.meta.dirname, "kit.css"), "utf8");
const rule = (sel: string) =>
  new RegExp(`${sel.replace(/[.*>[\]]/g, "\\$&")}\\s*\\{([^}]*)\\}`).exec(css)?.[1] ?? "";

describe("Banner", () => {
  it("without wrap, keeps one line: no data-wrap, the text in the tooltip", () => {
    render(<Banner tone="awaiting" title="Approval" text={SENTENCE} />);
    const banner = screen.getByRole("status");
    expect(banner).toHaveClass("d-banner");
    expect(banner).not.toHaveAttribute("data-wrap");
    expect(banner).toHaveAttribute("title", SENTENCE);
  });

  it("with wrap, marks itself and drops the tooltip: nothing is cut", () => {
    render(<Banner tone="awaiting" title={SENTENCE} text="Then save" wrap actions={<Button size="sm">Edit</Button>} />);
    const banner = screen.getByRole("status");
    expect(banner).toHaveAttribute("data-wrap", "true");
    expect(banner).not.toHaveAttribute("title");
    const parts = [...banner.children].map((c) => c.className.toString());
    expect(parts.slice(1)).toEqual(["d-banner__title", "d-banner__text", "d-banner__actions"]);
  });

  it("wrap and detail go together", () => {
    render(<Banner tone="failed" title="Failed" wrap detail="The whole reason" detailTestId="reason" />);
    const banner = screen.getByRole("alert");
    expect(banner).toHaveAttribute("data-wrap", "true");
    expect(banner).toHaveAttribute("data-detail", "true");
    expect(screen.getByTestId("reason")).toHaveTextContent("The whole reason");
  });

  it("kit.css wraps the title and the text under data-wrap, at body weight, with tokens", () => {
    const title = rule(".d-banner[data-wrap] .d-banner__title");
    expect(title).toMatch(/white-space:\s*normal/);
    expect(title).toMatch(/font-weight:\s*var\(--w-regular\)/);
    expect(title).toMatch(/min-width:\s*0/);
    const text = rule(".d-banner[data-wrap] .d-banner__text");
    expect(text).toMatch(/white-space:\s*normal/);
    expect(text).toMatch(/text-overflow:\s*clip/);
    expect(rule(".d-banner[data-wrap]")).toMatch(/padding-block:\s*var\(--g-2\)/);
    for (const r of [title, text]) expect(r).not.toMatch(/#[0-9a-f]{3,6}|\d+px/i);
    // The default stays one line.
    expect(rule(".d-banner__title")).toMatch(/white-space:\s*nowrap/);
    expect(rule(".d-banner__text")).toMatch(/text-overflow:\s*ellipsis/);
  });
});
