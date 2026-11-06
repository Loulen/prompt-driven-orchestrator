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
    expect(aside).toHaveClass("top-(--h-topbar)", "bottom-0", "right-0");
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

describe("FullWindowShell — a strip of tabs under the header", () => {
  function Strip() {
    const [active, setActive] = useState("a");
    return (
      <FullWindowShell
        title="Surface"
        subtitle={active === "a" ? "Alpha" : "Beta"}
        testId="surface"
        rail={[
          { id: "a", label: "Alpha", icon: <span data-testid="icon-a" /> },
          { id: "b", label: "Beta" },
        ]}
        railPlacement="top"
        railAside={<span data-testid="strip-aside">dates</span>}
        activeRail={active}
        onRailChange={setActive}
        railAriaLabel="Sections"
        railTestIdPrefix="surface-tab"
        onClose={() => {}}
        closeLabel="Close surface"
      >
        <p>Main</p>
      </FullWindowShell>
    );
  }

  it("draws the tabs with their icons, the aside at the end, and walks them with ← →", () => {
    render(<Strip />);
    const strip = screen.getByRole("tablist", { name: "Sections" });
    expect(screen.getByTestId("surface-header")).not.toContainElement(strip);
    expect(within(strip).getByTestId("icon-a")).toBeInTheDocument();
    expect(screen.getByTestId("strip-aside")).toBeInTheDocument();
    expect(screen.getByTestId("surface-header")).toHaveTextContent("SurfaceAlpha");

    fireEvent.keyDown(strip, { key: "ArrowRight" });
    expect(screen.getByTestId("surface-tab-b")).toHaveAttribute("aria-selected", "true");
    expect(screen.getByTestId("surface-header")).toHaveTextContent("SurfaceBeta");
    // ↑ ↓ belong to the left rail only.
    fireEvent.keyDown(strip, { key: "ArrowDown" });
    expect(screen.getByTestId("surface-tab-b")).toHaveAttribute("aria-selected", "true");
    fireEvent.keyDown(strip, { key: "ArrowLeft" });
    expect(screen.getByTestId("surface-tab-a")).toHaveAttribute("aria-selected", "true");
  });

  it("cuts a long tab name, named in full on hover, and scrolls the tab chosen into view", () => {
    const scrollIntoView = vi.fn();
    const original = Element.prototype.scrollIntoView;
    Element.prototype.scrollIntoView = scrollIntoView;
    try {
      render(<Strip />);
      const label = screen.getByText("Beta");
      expect(label).toHaveClass("truncate");
      expect(label).toHaveAttribute("title", "Beta");
      fireEvent.keyDown(screen.getByRole("tablist", { name: "Sections" }), { key: "ArrowRight" });
      expect((scrollIntoView.mock.instances.at(-1) as unknown as HTMLElement).dataset.testid).toBe("surface-tab-b");
    } finally {
      // jsdom ships none: put back its absence, not an own `undefined`.
      if (original) Element.prototype.scrollIntoView = original;
      else delete (Element.prototype as Partial<Element>).scrollIntoView;
    }
  });
});

describe("FullWindowShell — under the top bar (CORE-183)", () => {
  it("lays its layer under the app's top bar, never over it", () => {
    renderShell(null);
    const layer = screen.getByTestId("surface-layer");
    // The layer starts at the top bar's bottom edge: the bar stays on screen and clickable.
    expect(layer.className).toContain("top-(--h-topbar)");
    expect(layer.className).not.toContain("inset-0");
    expect(screen.getByTestId("surface").className).toContain("var(--h-topbar)");
    // The header keeps its ✕.
    expect(within(screen.getByTestId("surface-header")).getByRole("button", { name: "Close surface" })).toBeTruthy();
  });
});
