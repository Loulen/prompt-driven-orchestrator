import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import ThemeSelect from "./ThemeSelect";
import { initTheme } from "../../hooks/useTheme";

function stubOs(dark: boolean) {
  vi.stubGlobal("matchMedia", (query: string) => ({
    media: query,
    matches: dark,
    addEventListener: () => {},
    removeEventListener: () => {},
  }));
}

beforeEach(() => {
  localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
  stubOs(true);
  initTheme();
});

afterEach(() => {
  vi.unstubAllGlobals();
});

const option = (name: string) => screen.getByRole("radio", { name });

describe("ThemeSelect — choosing a theme (#759)", () => {
  it("offers light, dark and system under a named group", () => {
    render(<ThemeSelect />);
    expect(screen.getByRole("radiogroup", { name: /theme/i })).toBeInTheDocument();
    expect(screen.getAllByRole("radio")).toHaveLength(3);
  });

  it("checks the active preference and only that one (#781: dark by default)", () => {
    render(<ThemeSelect />);
    expect(option("Dark")).toHaveAttribute("aria-checked", "true");
    expect(option("Light")).toHaveAttribute("aria-checked", "false");
    expect(option("System")).toHaveAttribute("aria-checked", "false");
  });

  it("repaints the document and persists the choice on click", async () => {
    const user = userEvent.setup();
    render(<ThemeSelect />);
    await user.click(option("Light"));
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
    expect(localStorage.getItem("pdo.ui.theme")).toBe("light");
    expect(option("Light")).toHaveAttribute("aria-checked", "true");
    expect(option("System")).toHaveAttribute("aria-checked", "false");
  });

  it("names the theme system resolves to, so 'System' is not a blind choice", () => {
    render(<ThemeSelect />);
    expect(option("System")).toHaveAttribute("title", "System (dark)");
  });

  it("keeps naming the OS theme when another one is pinned", async () => {
    stubOs(false);
    initTheme();
    const user = userEvent.setup();
    render(<ThemeSelect />);
    await user.click(option("Dark"));
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    expect(option("System")).toHaveAttribute("title", "System (light)");
  });
});

describe("ThemeSelect — keyboard (#759)", () => {
  it("puts one tab stop on the group: only the checked option is tabbable", () => {
    render(<ThemeSelect />);
    expect(option("Dark")).toHaveAttribute("tabindex", "0");
    expect(option("Light")).toHaveAttribute("tabindex", "-1");
    expect(option("System")).toHaveAttribute("tabindex", "-1");
  });

  it("moves and selects with the arrow keys, wrapping around", async () => {
    const user = userEvent.setup();
    render(<ThemeSelect />);
    await user.tab();
    expect(option("Dark")).toHaveFocus();
    await user.keyboard("{ArrowRight}");
    expect(option("System")).toHaveAttribute("aria-checked", "true");
    expect(option("System")).toHaveFocus();
    await user.keyboard("{ArrowLeft}");
    expect(option("Dark")).toHaveAttribute("aria-checked", "true");
  });

  it("selects with Home and End", async () => {
    const user = userEvent.setup();
    render(<ThemeSelect />);
    await user.tab();
    await user.keyboard("{Home}");
    expect(option("System")).toHaveAttribute("aria-checked", "true");
    await user.keyboard("{End}");
    expect(option("Dark")).toHaveAttribute("aria-checked", "true");
  });
});
