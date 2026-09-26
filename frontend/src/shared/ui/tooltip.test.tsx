import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect } from "vitest";
import { Tooltip, TooltipProvider } from "./tooltip";

function renderWithProvider(ui: React.ReactNode) {
  return render(<TooltipProvider>{ui}</TooltipProvider>);
}

describe("Tooltip", () => {
  it("renders content on hover after delay", async () => {
    const user = userEvent.setup();
    renderWithProvider(
      <Tooltip content="Help text" delay={0}>
        <button>Hover me</button>
      </Tooltip>,
    );

    const trigger = screen.getByRole("button", { name: "Hover me" });
    await user.hover(trigger);

    await waitFor(() => {
      expect(screen.getByTestId("tooltip-content")).toHaveTextContent("Help text");
    });
  });

  it("hides on mouseout", async () => {
    const user = userEvent.setup();
    renderWithProvider(
      <Tooltip content="Help text" delay={0}>
        <button>Hover me</button>
      </Tooltip>,
    );

    const trigger = screen.getByRole("button", { name: "Hover me" });
    await user.hover(trigger);
    await waitFor(() => {
      expect(screen.getByTestId("tooltip-content")).toBeInTheDocument();
    });

    fireEvent.pointerDown(trigger);
    await waitFor(() => {
      expect(screen.queryByTestId("tooltip-content")).not.toBeInTheDocument();
    });
  });
});

// #397: Radix only ever wires `aria-describedby`, and only while the tooltip is
// open — so the wrapper has to supply the *name* for icon-only triggers.
describe("Tooltip accessible name (#397)", () => {
  it("names an icon-only button with the tooltip text, at rest", () => {
    renderWithProvider(
      <Tooltip content="Merge node">
        <button data-testid="t">
          <svg aria-hidden="true" />
        </button>
      </Tooltip>,
    );
    // No hover, no focus: the name must be there in the resting state.
    expect(screen.getByTestId("t")).toHaveAccessibleName("Merge node");
  });

  it("names a disabled icon-only button too", () => {
    renderWithProvider(
      <Tooltip content="Undo · Ctrl+Z">
        <button data-testid="t" disabled>
          <svg aria-hidden="true" />
        </button>
      </Tooltip>,
    );
    expect(screen.getByTestId("t")).toHaveAccessibleName("Undo · Ctrl+Z");
  });

  it("leaves a text button's visible label as its name (WCAG 2.5.3)", () => {
    renderWithProvider(
      <Tooltip content="A Node that forks a sub-worktree of its own.">
        <button data-testid="t">Isolated worktree</button>
      </Tooltip>,
    );
    expect(screen.getByTestId("t")).toHaveAccessibleName("Isolated worktree");
    expect(screen.getByTestId("t")).not.toHaveAttribute("aria-label");
  });

  it("sees text nested in host elements", () => {
    renderWithProvider(
      <Tooltip content="Tooltip prose">
        <button data-testid="t">
          <span>
            <svg aria-hidden="true" />
            Save
          </span>
        </button>
      </Tooltip>,
    );
    expect(screen.getByTestId("t")).toHaveAccessibleName("Save");
  });

  it("does not override an explicit aria-label", () => {
    renderWithProvider(
      <Tooltip content="When on, reads all iter-N files.">
        <button data-testid="t" role="switch" aria-checked={false} aria-label="repeated">
          <svg aria-hidden="true" />
        </button>
      </Tooltip>,
    );
    expect(screen.getByTestId("t")).toHaveAccessibleName("repeated");
  });

  it("does not override a title", () => {
    renderWithProvider(
      <Tooltip content="Tooltip prose">
        <button data-testid="t" title="Saved to library">
          <svg aria-hidden="true" />
        </button>
      </Tooltip>,
    );
    expect(screen.getByTestId("t")).toHaveAccessibleName("Saved to library");
  });

  it("leaves a non-interactive trigger alone — an aria-label there is inert", () => {
    renderWithProvider(
      <Tooltip content="Pooled input: several same-named edges land here.">
        <div data-testid="t">
          <svg aria-hidden="true" />
        </div>
      </Tooltip>,
    );
    expect(screen.getByTestId("t")).not.toHaveAttribute("aria-label");
  });

  it("names an element that declares a role", () => {
    renderWithProvider(
      <Tooltip content="Detach to OS terminal">
        <span data-testid="t" role="button">
          <svg aria-hidden="true" />
        </span>
      </Tooltip>,
    );
    expect(screen.getByTestId("t")).toHaveAccessibleName("Detach to OS terminal");
  });

  it("still shows the tooltip on hover once the trigger is named", async () => {
    const user = userEvent.setup();
    renderWithProvider(
      <Tooltip content="Pipeline info" delay={0}>
        <button data-testid="t">
          <svg aria-hidden="true" />
        </button>
      </Tooltip>,
    );
    await user.hover(screen.getByTestId("t"));
    await waitFor(() => {
      expect(screen.getByTestId("tooltip-content")).toHaveTextContent("Pipeline info");
    });
    // Name and description coexist: the name is stable, the description is not.
    expect(screen.getByTestId("t")).toHaveAccessibleName("Pipeline info");
  });
});
