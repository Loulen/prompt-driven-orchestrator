import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import ChildRunBar from "./ChildRunBar";

describe("the bar back to the parent run", () => {
  it.each([
    ["the arrow", "orchestrator-back"],
    ["the parent's name", null],
    ["the node's name", null],
  ])("leads back from a click on %s", (what, testId) => {
    const onBack = vi.fn();
    render(<ChildRunBar parentRunName="Audit the shop" nodeName="Dispatch fixes" onBack={onBack} />);
    const target =
      testId != null
        ? screen.getByTestId(testId)
        : screen.getByText(what === "the parent's name" ? "Audit the shop" : "Dispatch fixes");
    fireEvent.click(target);
    expect(onBack).toHaveBeenCalledOnce();
  });

  it("is one button, named in full in its tooltip", () => {
    render(<ChildRunBar parentRunName="Audit the shop" nodeName="Dispatch fixes" onBack={() => {}} />);
    const bar = screen.getByTestId("orchestrator-back-bar");
    expect(bar.tagName).toBe("BUTTON");
    expect(bar).toHaveAttribute("title", "Back to Audit the shop · Dispatch fixes");
  });
});
