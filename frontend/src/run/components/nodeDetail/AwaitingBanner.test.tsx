/**
 * ADR-0064: the banner of a parent node that waits on its child runs leads to
 * the Orchestration tab; once a child failed, it says so and leads to it.
 */
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import AwaitingBanner from "./AwaitingBanner";

const waiting = { cause: "children_pending", since: "2026-09-12T10:00:00Z" } as const;

describe("AwaitingBanner on children_pending", () => {
  it("says the node waits on its child runs and opens the Orchestration tab", () => {
    const onOpenOrchestration = vi.fn();
    render(<AwaitingBanner awaiting={waiting} terminalLive onOpenOrchestration={onOpenOrchestration} />);
    expect(screen.getByTestId("awaiting-banner")).toHaveAttribute("data-cause", "children_pending");
    expect(screen.getByTestId("awaiting-banner-title")).toHaveTextContent("Waiting on child runs");
    expect(screen.getByTestId("awaiting-banner-message")).toHaveTextContent("completes once every child run ends");
    fireEvent.click(screen.getByRole("button", { name: "Open Orchestration" }));
    expect(onOpenOrchestration).toHaveBeenCalledTimes(1);
  });

  it("offers no Open Orchestration without a tab to open", () => {
    render(<AwaitingBanner awaiting={waiting} terminalLive />);
    expect(screen.queryByTestId("awaiting-open-orchestration")).toBeNull();
  });

  it("turns failed with a failed child, Open leading to it", () => {
    const onOpenChildRun = vi.fn();
    render(
      <AwaitingBanner
        awaiting={waiting}
        terminalLive
        failedChildren={2}
        failedChildId="child-ko"
        onOpenChildRun={onOpenChildRun}
        onOpenOrchestration={() => {}}
      />,
    );
    expect(screen.getByTestId("awaiting-banner-title")).toHaveTextContent("2 child runs failed");
    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(screen.queryByTestId("awaiting-open-orchestration")).toBeNull();
    fireEvent.click(screen.getByTestId("awaiting-open-failed-child"));
    expect(onOpenChildRun).toHaveBeenCalledWith("child-ko");
  });
});
