import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import HarnessSelect from "./HarnessSelect";
import type { HarnessCatalog } from "../lib/harness";

const catalog: HarnessCatalog = {
  builtin: [
    { name: "claude", installed: true },
    { name: "opencode", installed: false },
  ],
  descriptors: [
    { name: "pi", installed: true },
    { name: "aider", installed: false },
  ],
};

function optionByText(select: HTMLSelectElement, text: string): HTMLOptionElement {
  const opt = Array.from(select.options).find((o) => o.textContent === text);
  if (!opt) throw new Error(`no option "${text}"`);
  return opt;
}

describe("HarnessSelect (#586)", () => {
  it("leads with the inherit sentinel, then the two named sections", () => {
    render(
      <HarnessSelect
        value=""
        onChange={() => {}}
        catalog={catalog}
        inheritLabel="Use instance default (claude)"
        data-testid="hs"
      />,
    );
    const select = screen.getByTestId("hs") as HTMLSelectElement;
    // The inherit option is first and carries the empty value.
    expect(select.options[0].value).toBe("");
    expect(select.options[0].textContent).toBe("Use instance default (claude)");
    // Two <optgroup> sections, Built-in before From descriptors.
    const groups = select.querySelectorAll("optgroup");
    expect(Array.from(groups).map((g) => g.getAttribute("label"))).toEqual([
      "Built-in",
      "From descriptors",
    ]);
  });

  it("greys and disables a harness whose binary is not installed", () => {
    render(
      <HarnessSelect value="" onChange={() => {}} catalog={catalog} inheritLabel="x" data-testid="hs" />,
    );
    const select = screen.getByTestId("hs") as HTMLSelectElement;
    // Installed → plain name, selectable.
    expect(optionByText(select, "claude").disabled).toBe(false);
    expect(optionByText(select, "pi").disabled).toBe(false);
    // Uninstalled → "not installed" note and non-selectable, in BOTH sections.
    expect(optionByText(select, "opencode — not installed").disabled).toBe(true);
    expect(optionByText(select, "aider — not installed").disabled).toBe(true);
  });

  it("reports the chosen harness by name", () => {
    const onChange = vi.fn();
    render(
      <HarnessSelect value="" onChange={onChange} catalog={catalog} inheritLabel="x" data-testid="hs" />,
    );
    fireEvent.change(screen.getByTestId("hs"), { target: { value: "pi" } });
    expect(onChange).toHaveBeenCalledWith("pi");
  });

  it("omits an empty section rather than rendering a blank group", () => {
    render(
      <HarnessSelect
        value=""
        onChange={() => {}}
        catalog={{ builtin: [{ name: "claude", installed: true }], descriptors: [] }}
        inheritLabel="x"
        data-testid="hs"
      />,
    );
    const groups = (screen.getByTestId("hs") as HTMLSelectElement).querySelectorAll("optgroup");
    expect(Array.from(groups).map((g) => g.getAttribute("label"))).toEqual(["Built-in"]);
  });
});
