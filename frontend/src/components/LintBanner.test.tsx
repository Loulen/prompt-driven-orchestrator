import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import LintBanner, { type LintBannerItem } from "./LintBanner";

const lint = (message: string, i = 0): LintBannerItem => ({
  id: `lint:${i}`,
  kind: "lint",
  message,
});
const nudge = (id: string, message: string): LintBannerItem => ({
  id,
  kind: "nudge",
  message,
});

const noop = () => {};

describe("LintBanner", () => {
  it("renders nothing when items array is empty", () => {
    const { container } = render(<LintBanner items={[]} onDismiss={noop} />);
    expect(container.firstChild).toBeNull();
  });

  it("renders diagnostic messages", () => {
    render(
      <LintBanner
        items={[lint("node 'reviewer' receives edges from 2 code-mutating nodes without a Merge")]}
        onDismiss={noop}
      />,
    );
    expect(screen.getByText(/code-mutating nodes without a Merge/)).toBeInTheDocument();
  });

  it("renders multiple diagnostics", () => {
    render(
      <LintBanner items={[lint("first warning", 0), lint("second warning", 1)]} onDismiss={noop} />,
    );
    expect(screen.getByText("first warning")).toBeInTheDocument();
    expect(screen.getByText("second warning")).toBeInTheDocument();
  });

  it("has the lint-banner testid", () => {
    render(<LintBanner items={[lint("some warning")]} onDismiss={noop} />);
    expect(screen.getByTestId("lint-banner")).toBeInTheDocument();
  });

  it("renders a dismiss × on a nudge row but NOT on a lint row (#268)", () => {
    render(
      <LintBanner
        items={[lint("a correctness warning"), nudge("fanout:worker", "consider fanning out")]}
        onDismiss={noop}
      />,
    );
    // The nudge row carries the dismiss button…
    expect(screen.getByTestId("lint-banner-dismiss-fanout:worker")).toBeInTheDocument();
    // …and it is the ONLY dismiss affordance (lint rows have none).
    expect(screen.getAllByLabelText("Dismiss suggestion")).toHaveLength(1);
  });

  it("fires onDismiss with the row's id when the × is clicked (#268)", () => {
    const onDismiss = vi.fn();
    render(
      <LintBanner items={[nudge("fanout:worker", "consider fanning out")]} onDismiss={onDismiss} />,
    );
    fireEvent.click(screen.getByTestId("lint-banner-dismiss-fanout:worker"));
    expect(onDismiss).toHaveBeenCalledExactlyOnceWith("fanout:worker");
  });
});
