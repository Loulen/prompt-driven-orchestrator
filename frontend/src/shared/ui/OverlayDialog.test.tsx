import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import OverlayDialog from "./OverlayDialog";

describe("OverlayDialog", () => {
  it("focuses the control marked data-autofocus", () => {
    render(
      <OverlayDialog title="New Run" onClose={vi.fn()}>
        <input aria-label="first" />
        <input aria-label="name" data-autofocus="" />
      </OverlayDialog>,
    );
    expect(screen.getByLabelText("name")).toHaveFocus();
  });

  it("falls back to the body's first control when the autofocus target is disabled", () => {
    render(
      <OverlayDialog title="New Run" onClose={vi.fn()}>
        <input aria-label="name" data-autofocus="" disabled />
        <input type="checkbox" aria-label="auto" />
      </OverlayDialog>,
    );
    expect(screen.getByLabelText("auto")).toHaveFocus();
  });

  it("hands the focus back to the opener on close", () => {
    const opener = document.createElement("button");
    document.body.appendChild(opener);
    opener.focus();
    const { unmount } = render(
      <OverlayDialog title="New Run" onClose={vi.fn()}>
        <input aria-label="name" data-autofocus="" />
      </OverlayDialog>,
    );
    expect(opener).not.toHaveFocus();
    unmount();
    expect(opener).toHaveFocus();
    opener.remove();
  });
});
