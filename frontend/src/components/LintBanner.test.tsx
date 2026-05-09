import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import LintBanner from "./LintBanner";

describe("LintBanner", () => {
  it("renders nothing when diagnostics array is empty", () => {
    const { container } = render(<LintBanner diagnostics={[]} />);
    expect(container.firstChild).toBeNull();
  });

  it("renders diagnostic messages", () => {
    const diagnostics = [
      "node 'reviewer' receives edges from 2 code-mutating nodes without a Merge",
    ];
    render(<LintBanner diagnostics={diagnostics} />);
    expect(screen.getByText(/code-mutating nodes without a Merge/)).toBeInTheDocument();
  });

  it("renders multiple diagnostics", () => {
    const diagnostics = [
      "first warning",
      "second warning",
    ];
    render(<LintBanner diagnostics={diagnostics} />);
    expect(screen.getByText("first warning")).toBeInTheDocument();
    expect(screen.getByText("second warning")).toBeInTheDocument();
  });

  it("has the lint-banner testid", () => {
    render(<LintBanner diagnostics={["some warning"]} />);
    expect(screen.getByTestId("lint-banner")).toBeInTheDocument();
  });
});
