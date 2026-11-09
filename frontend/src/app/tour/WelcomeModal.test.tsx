/**
 * The welcome modal under each Marque (CORE-225): the lockup of the Marque on
 * top, « by Acogni » under PDO's (under Acogni Driver's the wordmark says it),
 * the focus kept on Full tour.
 */
import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import WelcomeModal from "./WelcomeModal";

function renderModal() {
  render(<WelcomeModal onStartFullTour={vi.fn()} onStartTour={vi.fn()} onLater={vi.fn()} />);
}

afterEach(() => {
  delete document.documentElement.dataset.brand;
});

describe("the welcome modal", () => {
  it("welcomes to PDO with its lockup, signed by Acogni, on the vanilla binary", () => {
    renderModal();
    expect(screen.getByRole("dialog", { name: "Welcome to PDO" })).toBeInTheDocument();
    expect(screen.getByTestId("brand-lockup")).toHaveAttribute("aria-label", "PDO");
    expect(screen.getByTestId("by-acogni")).toHaveTextContent("by Acogni");
    expect(screen.getByTestId("tour-welcome")).not.toHaveTextContent("Acogni Driver");
    expect(screen.getByTestId("tour-welcome-full")).toHaveFocus();
  });

  it("welcomes to Acogni Driver with its lockup, and no second signature", () => {
    document.documentElement.dataset.brand = "acogni-driver";
    renderModal();
    expect(screen.getByRole("dialog", { name: "Welcome to Acogni Driver" })).toBeInTheDocument();
    expect(screen.getByTestId("acogni-lockup")).toHaveAttribute("aria-label", "Acogni Driver");
    expect(screen.queryByTestId("by-acogni")).toBeNull();
  });
});
