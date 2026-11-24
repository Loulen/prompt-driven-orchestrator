import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import HarnessLogo from "./HarnessLogo";

describe("HarnessLogo", () => {
  it("draws the mark of each first-party harness", () => {
    for (const harness of ["claude", "copilot", "opencode", "pi"]) {
      const { container, unmount } = render(<HarnessLogo harness={harness} />);
      const logo = container.querySelector(`[data-harness-logo="${harness}"]`);
      expect(logo?.tagName.toLowerCase()).toBe("svg");
      expect(logo?.querySelectorAll("path").length).toBeGreaterThan(0);
      unmount();
    }
  });

  it("falls back to the initial of a harness without a mark", () => {
    const { container } = render(<HarnessLogo harness="ghost" size={16} />);
    const logo = container.querySelector('[data-harness-logo="ghost"]');
    expect(logo?.tagName.toLowerCase()).toBe("span");
    expect(logo).toHaveTextContent("G");
  });

  it("is decorative unless named", () => {
    const { container } = render(<HarnessLogo harness="claude" />);
    expect(container.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
    render(<HarnessLogo harness="pi" label="pi" />);
    expect(screen.getByRole("img", { name: "pi" })).toBeInTheDocument();
  });
});
