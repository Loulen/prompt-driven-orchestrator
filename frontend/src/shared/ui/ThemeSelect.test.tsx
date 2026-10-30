import { describe, it, expect, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import ThemeSelect from "./ThemeSelect";
import { initTheme } from "../../hooks/useTheme";

beforeEach(() => {
  localStorage.clear();
  initTheme();
});

const option = (name: string) => screen.getByRole("radio", { name });

describe("ThemeSelect: choosing a Theme (ADR-0093)", () => {
  it("offers Acogni and Classic under a named group, Acogni checked by default", () => {
    render(<ThemeSelect />);
    expect(screen.getByRole("radiogroup", { name: "Theme" })).toBeInTheDocument();
    expect(screen.getAllByRole("radio").map((r) => r.textContent)).toEqual(["Acogni", "Classic"]);
    expect(option("Acogni")).toHaveAttribute("aria-checked", "true");
    expect(option("Classic")).toHaveAttribute("aria-checked", "false");
  });

  it("repaints the document and persists the choice on click, the mode untouched", async () => {
    const user = userEvent.setup();
    render(<ThemeSelect />);
    await user.click(option("Classic"));
    expect(document.documentElement.getAttribute("data-theme")).toBe("classic");
    expect(document.documentElement.getAttribute("data-mode")).toBe("dark");
    expect(localStorage.getItem("pdo.ui.theme")).toBe("classic");
    expect(option("Classic")).toHaveAttribute("aria-checked", "true");
  });

  it("puts one tab stop on the group and moves with the arrows", async () => {
    const user = userEvent.setup();
    render(<ThemeSelect />);
    expect(option("Acogni")).toHaveAttribute("tabindex", "0");
    expect(option("Classic")).toHaveAttribute("tabindex", "-1");
    await user.tab();
    await user.keyboard("{ArrowRight}");
    expect(option("Classic")).toHaveAttribute("aria-checked", "true");
    expect(option("Classic")).toHaveFocus();
  });
});
