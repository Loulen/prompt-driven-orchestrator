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
