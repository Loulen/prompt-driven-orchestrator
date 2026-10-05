import { fireEvent, render, screen, within } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import FullWindowShell, { type ShellDrawer } from "./FullWindowShell";

function renderShell(drawer: ShellDrawer | null, onEscape = vi.fn()) {
  const onClose = vi.fn();
  render(
    <FullWindowShell
      title="Surface"
      testId="surface"
      rail={[{ id: "a", label: "A" }]}
      activeRail="a"
      onRailChange={() => {}}
      railAriaLabel="Sections"
      railTestIdPrefix="surface-tab"
      onEscape={onEscape}
      onClose={onClose}
      closeLabel="Close surface"
      drawer={drawer}
    >
      <p data-testid="surface-main">Main</p>
    </FullWindowShell>,
  );
  return { onClose, onEscape };
}

function panel(overrides: Partial<ShellDrawer> = {}): ShellDrawer {
  return {
    title: "Panel",
    escHint: "Esc returns to Surface",
    widthClassName: "w-[min(420px,90vw)]",
    testId: "surface-panel",
    onClose: vi.fn(),
    content: <p>Panel body</p>,
    ...overrides,
  };
}

describe("FullWindowShell — secondary panel frame (#944)", () => {
  it("renders nothing without a panel", () => {
    renderShell(null);
    expect(screen.queryByRole("complementary")).not.toBeInTheDocument();
  });

  it("frames the panel under the header with its title, actions, Escape hint and ✕", () => {
    renderShell(
      panel({
        actions: <button type="button">Act</button>,
        dataAttributes: { "data-drawer": "x" },
      }),
    );
    const aside = screen.getByTestId("surface-panel");
    expect(aside).toHaveClass("top-14", "bottom-0", "right-0");
    expect(aside).toHaveClass("w-[min(420px,90vw)]");
    expect(aside).toHaveAttribute("data-drawer", "x");
    expect(within(aside).getByRole("heading", { name: "Panel" })).toBeInTheDocument();
    expect(aside).toHaveTextContent("Esc returns to Surface");
    expect(within(aside).getByRole("button", { name: "Act" })).toBeInTheDocument();
    expect(aside).toHaveTextContent("Panel body");
    // The surface's own ✕ stays available next to the panel's.
    expect(screen.getByRole("button", { name: "Close surface" })).toBeInTheDocument();
  });

  it("closes through its ✕, named from the testId unless told otherwise", () => {
    const onClose = vi.fn();
    const { onClose: onSurfaceClose } = renderShell(panel({ onClose }));
    fireEvent.click(screen.getByTestId("surface-panel-close"));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onSurfaceClose).not.toHaveBeenCalled();
    expect(screen.getByTestId("surface-panel-close")).toHaveAccessibleName("Close panel");
  });

  it("honours a custom close testId", () => {
    renderShell(panel({ closeTestId: "custom-close" }));
    expect(screen.getByTestId("custom-close")).toBeInTheDocument();
  });

  it("does not close on a click in the main pane; Escape goes to the host", () => {
    const onClose = vi.fn();
    const { onEscape } = renderShell(panel({ onClose }));
    fireEvent.click(screen.getByTestId("surface-main"));
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onEscape).toHaveBeenCalledTimes(1);
  });

  describe("focus return", () => {
    function Harness() {
      const [open, setOpen] = useState(false);
      return (
        <FullWindowShell
          title="Surface"
          testId="surface"
          rail={[{ id: "a", label: "A" }]}
          activeRail="a"
          onRailChange={() => {}}
          railAriaLabel="Sections"
          railTestIdPrefix="surface-tab"
          onEscape={() => setOpen(false)}
          onClose={() => {}}
          closeLabel="Close surface"
          headerActions={
            <button type="button" data-testid="opener" onClick={() => setOpen((v) => !v)}>
              Open
            </button>
          }
          drawer={open ? panel({ onClose: () => setOpen(false) }) : null}
        >
          <button type="button" data-testid="elsewhere">
            Elsewhere
          </button>
        </FullWindowShell>
      );
    }

    it("gives focus back to the opener when the panel closes on its ✕ or Escape", () => {
      render(<Harness />);
      const opener = screen.getByTestId("opener");
      opener.focus();
      fireEvent.click(opener);
      screen.getByTestId("surface-panel-close").focus();
      fireEvent.click(screen.getByTestId("surface-panel-close"));
      expect(screen.queryByTestId("surface-panel")).not.toBeInTheDocument();
      expect(opener).toHaveFocus();

      fireEvent.click(opener);
      (document.activeElement as HTMLElement).blur();
      fireEvent.keyDown(window, { key: "Escape" });
      expect(opener).toHaveFocus();
    });

    it("leaves focus alone when the user moved it elsewhere", () => {
      render(<Harness />);
      const opener = screen.getByTestId("opener");
      opener.focus();
      fireEvent.click(opener);
      screen.getByTestId("elsewhere").focus();
      fireEvent.keyDown(window, { key: "Escape" });
      expect(screen.queryByTestId("surface-panel")).not.toBeInTheDocument();
      expect(screen.getByTestId("elsewhere")).toHaveFocus();
    });
  });
});
