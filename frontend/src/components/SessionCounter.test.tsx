import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import SessionCounter from "./SessionCounter";

describe("SessionCounter", () => {
  it("shows the live session count over the cap", () => {
    render(<SessionCounter live={3} cap={10} />);
    expect(screen.getByTestId("session-counter")).toHaveTextContent("3 / 10");
  });

  it("uses the normal treatment when comfortably below the cap", () => {
    render(<SessionCounter live={3} cap={10} />);
    expect(screen.getByTestId("session-counter")).toHaveAttribute(
      "data-near-cap",
      "false",
    );
  });

  it("shifts to a warning treatment one slot before the cap", () => {
    render(<SessionCounter live={9} cap={10} />);
    expect(screen.getByTestId("session-counter")).toHaveAttribute(
      "data-near-cap",
      "true",
    );
  });

  it("keeps the warning treatment once the cap is full", () => {
    render(<SessionCounter live={10} cap={10} />);
    expect(screen.getByTestId("session-counter")).toHaveAttribute(
      "data-near-cap",
      "true",
    );
  });
});
