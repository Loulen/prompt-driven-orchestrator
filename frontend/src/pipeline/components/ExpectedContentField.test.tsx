import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import ExpectedContentField from "./ExpectedContentField";

function open() {
  render(
    <>
      <ExpectedContentField value="" onChange={vi.fn()} />
      <button type="button" data-testid="elsewhere">
        elsewhere
      </button>
    </>,
  );
  fireEvent.click(screen.getByRole("button", { name: "Add expected content" }));
  expect(screen.getByRole("textbox", { name: "Expected content" })).toBeInTheDocument();
}

describe("ExpectedContentField", () => {
  it("folds on a press outside it", () => {
    open();
    fireEvent.mouseDown(screen.getByTestId("elsewhere"));
    expect(screen.queryByRole("textbox", { name: "Expected content" })).not.toBeInTheDocument();
  });

  /**
   * FP of CORE-227 (F3-10a): a tour card's buttons leave the focus in the field
   * (their press is default-prevented). Folding anyway moved the card up under
   * the press, and the release missed Next. A press that keeps the focus here
   * keeps the field open.
   */
  it("stays open on a press that leaves the focus where it is", () => {
    open();
    const elsewhere = screen.getByTestId("elsewhere");
    elsewhere.addEventListener("mousedown", (e) => e.preventDefault());
    fireEvent.mouseDown(elsewhere);
    expect(screen.getByRole("textbox", { name: "Expected content" })).toBeInTheDocument();
  });
});
