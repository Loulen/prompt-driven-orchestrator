import { afterEach, describe, expect, it } from "vitest";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import InterfaceSection from "../settings/components/settingsSurface/InterfaceSection";
import { SETTINGS_CATEGORIES } from "../settings/model/settingsCategories";
import { DEFAULT_UI_SCALE, initUiScale, loadUiScale, setUiScale, UI_SCALE_STORAGE_KEY } from "./uiScale";

const zoom = () => document.documentElement.style.getPropertyValue("--app-zoom");

afterEach(() => {
  localStorage.clear();
  act(() => setUiScale(DEFAULT_UI_SCALE));
  document.documentElement.style.removeProperty("--app-zoom");
});

describe("UI scale", () => {
  it("is 120 % by default, and for anything unknown", () => {
    expect(loadUiScale()).toBe(120);
    localStorage.setItem(UI_SCALE_STORAGE_KEY, "95");
    expect(loadUiScale()).toBe(120);
    localStorage.setItem(UI_SCALE_STORAGE_KEY, "100");
    expect(loadUiScale()).toBe(100);
  });

  it("paints the stored scale on <html> at boot", () => {
    localStorage.setItem(UI_SCALE_STORAGE_KEY, "110");
    initUiScale();
    expect(zoom()).toBe("1.1");
  });

  it("is saved and painted at the change", () => {
    setUiScale(100);
    expect(localStorage.getItem(UI_SCALE_STORAGE_KEY)).toBe("100");
    expect(zoom()).toBe("1");
  });

  it("is chosen in Settings › Interface", async () => {
    const user = userEvent.setup();
    const interfaceSection = SETTINGS_CATEGORIES.flatMap((c) => c.sections).find((s) => s.label === "Interface");
    render(<InterfaceSection section={interfaceSection!} />);
    const group = screen.getByRole("radiogroup", { name: "UI scale" });
    expect(screen.getByRole("radio", { name: "120%" })).toHaveAttribute("aria-checked", "true");
    await user.click(screen.getByRole("radio", { name: "100%" }));
    expect(zoom()).toBe("1");
    expect(group).toContainElement(screen.getByRole("radio", { name: "100%" }));
    expect(screen.getByRole("radio", { name: "100%" })).toHaveAttribute("aria-checked", "true");
  });
});
