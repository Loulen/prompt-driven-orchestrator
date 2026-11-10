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
  it("folds on a click outside it", () => {
    open();
    const elsewhere = screen.getByTestId("elsewhere");
    fireEvent.mouseDown(elsewhere);
    fireEvent.mouseUp(elsewhere);
    fireEvent.click(elsewhere);
    expect(screen.queryByRole("textbox", { name: "Expected content" })).not.toBeInTheDocument();
  });

  /**
   * FP of CORE-227 (M3): folding on the press moved « + field », just below,
   * up before the release, and the click missed it. The field stays open
   * until the click outside is over.
   */
  it("folds on a click outside whose propagation is stopped", () => {
    open();
    const elsewhere = screen.getByTestId("elsewhere");
    elsewhere.addEventListener("click", (e) => e.stopPropagation());
    fireEvent.mouseDown(elsewhere);
    fireEvent.click(elsewhere);
    expect(screen.queryByRole("textbox", { name: "Expected content" })).not.toBeInTheDocument();
  });

  it("stays open until the click outside is released", () => {
    open();
    fireEvent.mouseDown(screen.getByTestId("elsewhere"));
    expect(screen.getByRole("textbox", { name: "Expected content" })).toBeInTheDocument();
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
    fireEvent.mouseUp(elsewhere);
    fireEvent.click(elsewhere);
    expect(screen.getByRole("textbox", { name: "Expected content" })).toBeInTheDocument();
  });
});
