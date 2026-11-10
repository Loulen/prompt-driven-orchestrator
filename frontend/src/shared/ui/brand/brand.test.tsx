/**
 * The Marque of the binary, as the front reads it (CORE-225): the daemon writes
 * `data-brand` on `<html>` before the bundle loads; the marks, the lockup and
 * the « by Acogni » signature draw from it.
 */
import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { brandName, currentBrand } from "./brand";
import { BrandMark } from "./BrandMark";
import { BrandLockup } from "./BrandLockup";
import { BySignature } from "./BySignature";
import { SettingsIcon } from "./AccentIcons";

function servedAs(id: string | null) {
  if (id == null) delete document.documentElement.dataset.brand;
  else document.documentElement.dataset.brand = id;
}

afterEach(() => servedAs(null));

describe("the Marque the page was served under", () => {
  it("is PDO when the page names none, or one the core does not know", () => {
    expect(currentBrand()).toEqual({ id: "pdo", name: "PDO" });
    servedAs("acme");
    expect(brandName()).toBe("PDO");
    servedAs("toString");
    expect(brandName()).toBe("PDO");
  });

  it("is Acogni Driver when the daemon declares it", () => {
    servedAs("acogni-driver");
    expect(currentBrand()).toEqual({ id: "acogni-driver", name: "Acogni Driver" });
  });
});

describe("BrandMark", () => {
  it("draws PDO's steps, an image named PDO, its accent kept ruby under every Theme", () => {
    render(<BrandMark size={16} />);
    const mark = screen.getByRole("img", { name: "PDO" });
    expect(mark).toHaveAttribute("data-brand", "pdo");
    expect(mark).toHaveAttribute("width", "16");
    expect(mark).toHaveAttribute("viewBox", "0 0 64 64");
    expect(mark.getAttribute("class")).toContain("acogni-brand");
    expect(mark.querySelector('[fill="var(--primary)"]')).not.toBeNull();
    // Decorative: never a control.
    expect(mark).not.toHaveAttribute("tabindex");
  });

  it("hides from assistive technologies when the name is written next to it", () => {
    render(<BrandMark decorative />);
    expect(screen.queryByRole("img")).toBeNull();
    expect(screen.getByTestId("brand-mark")).toHaveAttribute("aria-hidden", "true");
  });

  it("draws the A of Acogni under Acogni Driver", () => {
    servedAs("acogni-driver");
    render(<BrandMark />);
    const mark = screen.getByRole("img", { name: "Acogni Driver" });
    expect(mark).toHaveAttribute("data-brand", "acogni-driver");
    expect(mark).toHaveAttribute("viewBox", "0 -1460 1520 1520");
  });
});

describe("BrandLockup", () => {
  it("is PDO's mark and its name under PDO", () => {
    render(<BrandLockup />);
    const lockup = screen.getByRole("img", { name: "PDO" });
    expect(lockup).toHaveAttribute("data-testid", "brand-lockup");
    expect(lockup).toHaveTextContent("PDO");
    expect(lockup.querySelector(".tr-lockup__mark.acogni-brand")).not.toBeNull();
  });

  it("is the Acogni wordmark and its Driver tag under Acogni Driver, room left under the g", () => {
    servedAs("acogni-driver");
    render(<BrandLockup />);
    const lockup = screen.getByRole("img", { name: "Acogni Driver" });
    expect(lockup).toHaveAttribute("data-testid", "acogni-lockup");
    expect(lockup).toHaveTextContent("Driver");
    expect(lockup.querySelector("svg")).toHaveAttribute("viewBox", "-60 -1540 6970 2100");
  });
});

describe("the gear of Acogni", () => {
  it("signs « by Acogni » as text, never a link", () => {
    render(<BySignature />);
    const signature = screen.getByTestId("by-acogni");
    expect(signature).toHaveTextContent("by Acogni");
    expect(signature.querySelector("a")).toBeNull();
    expect(signature.querySelector('[data-testid="acogni-gear"]')).not.toBeNull();
  });

  it("is the Settings icon, its triangle in the ruby under every Theme", () => {
    render(<SettingsIcon size={14} strokeWidth={1.5} aria-hidden />);
    const gear = screen.getByTestId("acogni-gear");
    expect(gear).toHaveAttribute("width", "14");
    expect(gear.getAttribute("class")).toContain("acogni-brand");
    expect(gear.querySelector('[stroke="var(--primary)"]')).not.toBeNull();
  });
});
