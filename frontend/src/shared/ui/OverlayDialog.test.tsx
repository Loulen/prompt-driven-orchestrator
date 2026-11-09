import { describe, it, expect, vi } from "vitest";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Plus } from "lucide-react";
import { IconButton } from "./kit";
import { useState } from "react";
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

  // The form renders its first state, then the opening intent swaps a section
  // (the Run name for a Trigger's) or disables the focused control: the focus
  // must not fall to the page, where Tab, Escape and Ctrl+Enter miss the dialog.
  /**
   * FP of CORE-227: after Launch closed New Run, a « New run » tooltip stood
   * over the Run row the tour had just lit. The focus handed back to the opener
   * is the dialog's doing, not a keyboard user's: it opens no tooltip.
   */
  it("hands the focus back without opening the opener's tooltip", async () => {
    function Host() {
      const [open, setOpen] = useState(false);
      return (
        <>
          <IconButton icon={Plus} label="New run" onClick={() => setOpen(true)} />
          {open && (
            <OverlayDialog title="New Run" onClose={() => setOpen(false)}>
              <button type="button" onClick={() => setOpen(false)}>
                Run
              </button>
            </OverlayDialog>
          )}
        </>
      );
    }
    const user = userEvent.setup();
    render(<Host />);
    await user.click(screen.getByRole("button", { name: "New run" }));
    await user.click(screen.getByRole("button", { name: "Run" }));
    expect(screen.getByRole("button", { name: "New run" })).toHaveFocus();
    await act(() => new Promise((resolve) => setTimeout(resolve, 450)));
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("takes the focus back when the focused control goes away after the opening", async () => {
    let swap!: () => void;
    function Form() {
      const [mode, setMode] = useState<"run" | "trigger">("run");
      swap = () => setMode("trigger");
      return mode === "run" ? <input key="run" aria-label="run name" data-autofocus="" /> : <input key="trigger" aria-label="trigger name" data-autofocus="" />;
    }
    render(
      <OverlayDialog title="Edit Trigger" onClose={vi.fn()}>
        <Form />
      </OverlayDialog>,
    );
    expect(screen.getByLabelText("run name")).toHaveFocus();
    await act(async () => swap());
    expect(screen.getByLabelText("trigger name")).toHaveFocus();
  });

  it("takes the focus back when the focused control is disabled after the opening", async () => {
    let disable!: () => void;
    function Form() {
      const [auto, setAuto] = useState(false);
      disable = () => setAuto(true);
      return (
        <>
          <input aria-label="name" data-autofocus="" disabled={auto} />
          <input type="checkbox" aria-label="auto" />
        </>
      );
    }
    render(
      <OverlayDialog title="New Run" onClose={vi.fn()}>
        <Form />
      </OverlayDialog>,
    );
    expect(screen.getByLabelText("name")).toHaveFocus();
    await act(async () => disable());
    expect(screen.getByLabelText("auto")).toHaveFocus();
  });

  it("leaves the focus where the user put it", async () => {
    let add!: () => void;
    function Form() {
      const [more, setMore] = useState(false);
      add = () => setMore(true);
      return (
        <>
          <input aria-label="name" data-autofocus="" />
          <input aria-label="other" />
          {more && <span>loaded</span>}
        </>
      );
    }
    render(
      <OverlayDialog title="New Run" onClose={vi.fn()}>
        <Form />
      </OverlayDialog>,
    );
    screen.getByLabelText("other").focus();
    await act(async () => add());
    expect(screen.getByLabelText("other")).toHaveFocus();
  });
});
