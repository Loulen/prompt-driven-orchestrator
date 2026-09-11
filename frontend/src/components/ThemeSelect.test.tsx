import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import ThemeSelect from "./ThemeSelect";
import { initTheme } from "../hooks/useTheme";

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

describe("ThemeSelect — choosing a theme (#759)", () => {
  it("offers light, dark and system under a named group", () => {
    render(<ThemeSelect />);
    expect(screen.getByRole("radiogroup", { name: /theme/i })).toBeInTheDocument();
    expect(screen.getAllByRole("radio")).toHaveLength(3);
  });

  it("checks the active preference and only that one", () => {
    render(<ThemeSelect />);
    expect(screen.getByTestId("theme-option-system")).toHaveAttribute("aria-checked", "true");
    expect(screen.getByTestId("theme-option-light")).toHaveAttribute("aria-checked", "false");
    expect(screen.getByTestId("theme-option-dark")).toHaveAttribute("aria-checked", "false");
  });

  it("repaints the document and persists the choice on click", async () => {
    const user = userEvent.setup();
    render(<ThemeSelect />);
    await user.click(screen.getByTestId("theme-option-light"));
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
    expect(localStorage.getItem("pdo.ui.theme")).toBe("light");
    expect(screen.getByTestId("theme-option-light")).toHaveAttribute("aria-checked", "true");
    expect(screen.getByTestId("theme-option-system")).toHaveAttribute("aria-checked", "false");
  });

  it("names the theme system resolves to, so 'System' is not a blind choice", () => {
    render(<ThemeSelect />);
    expect(screen.getByTestId("theme-option-system")).toHaveTextContent(/dark/i);
  });

  it("keeps naming the OS theme when another one is pinned", async () => {
    stubOs(false);
    initTheme();
    const user = userEvent.setup();
    render(<ThemeSelect />);
    await user.click(screen.getByTestId("theme-option-dark"));
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    expect(screen.getByTestId("theme-option-system")).toHaveTextContent(/light/i);
    expect(screen.getByTestId("theme-option-system")).not.toHaveTextContent(/dark/i);
  });
});

describe("ThemeSelect — keyboard (#759)", () => {
  it("puts one tab stop on the group: only the checked option is tabbable", () => {
    render(<ThemeSelect />);
    expect(screen.getByTestId("theme-option-system")).toHaveAttribute("tabindex", "0");
    expect(screen.getByTestId("theme-option-light")).toHaveAttribute("tabindex", "-1");
    expect(screen.getByTestId("theme-option-dark")).toHaveAttribute("tabindex", "-1");
  });

  it("moves and selects with the arrow keys, wrapping around", async () => {
    const user = userEvent.setup();
    render(<ThemeSelect />);
    await user.tab();
    expect(screen.getByTestId("theme-option-system")).toHaveFocus();
    await user.keyboard("{ArrowRight}");
    expect(screen.getByTestId("theme-option-light")).toHaveAttribute("aria-checked", "true");
    expect(screen.getByTestId("theme-option-light")).toHaveFocus();
    await user.keyboard("{ArrowLeft}");
    expect(screen.getByTestId("theme-option-system")).toHaveAttribute("aria-checked", "true");
  });

  it("selects with Home and End", async () => {
    const user = userEvent.setup();
    render(<ThemeSelect />);
    await user.tab();
    await user.keyboard("{Home}");
    expect(screen.getByTestId("theme-option-light")).toHaveAttribute("aria-checked", "true");
    await user.keyboard("{End}");
    expect(screen.getByTestId("theme-option-system")).toHaveAttribute("aria-checked", "true");
  });
});
