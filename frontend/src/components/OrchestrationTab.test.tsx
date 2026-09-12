// #723/#783 — the Orchestration tab's shared pastilles: four pills (finished /
// failed / stale / running), zero pills hidden, the cluster absent without a child.
import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { ChildCountPills, OrchestrationTab } from "./OrchestrationTab";
import { countChildren, type RunChildEntry } from "../lib/orchestration";

const child = (id: string, over: Partial<RunChildEntry> = {}): RunChildEntry => ({
  run_id: id,
  pipeline_name: "implement",
  name: id,
  status: "running",
  ...over,
});

describe("ChildCountPills (#783)", () => {
  it("renders the four pills, in order, and hides a zero one", () => {
    render(<ChildCountPills counts={{ finished: 2, failed: 1, stale: 1, running: 0 }} />);
    expect(screen.getByTestId("child-count-pills-finished")).toHaveTextContent("2");
    expect(screen.getByTestId("child-count-pills-failed")).toHaveTextContent("1");
    expect(screen.getByTestId("child-count-pills-stale")).toHaveTextContent("1");
    expect(screen.queryByTestId("child-count-pills-running")).not.toBeInTheDocument();
    // Stale wears the same amber as the list's stalled dot.
    expect(screen.getByTestId("child-count-pills-stale").querySelector(".bg-st-stale")).not.toBeNull();
    expect(screen.getByTestId("child-count-pills")).toHaveAttribute(
      "aria-label",
      "2 finished, 1 failed, 1 stale, 0 running child runs",
    );
  });

  it("renders nothing at all without a child", () => {
    render(<ChildCountPills counts={{ finished: 0, failed: 0, stale: 0, running: 0 }} />);
    expect(screen.queryByTestId("child-count-pills")).not.toBeInTheDocument();
  });

  it("is a click target that swallows the click when onClick is given", () => {
    const onClick = vi.fn();
    const rowClick = vi.fn();
    render(
      <div onClick={rowClick}>
        <ChildCountPills counts={{ finished: 0, failed: 0, stale: 1, running: 0 }} onClick={onClick} clickHint="click to expand" />
      </div>,
    );
    expect(screen.getByTestId("child-count-pills-stale")).toHaveAttribute("title", "1 stale child run — click to expand");
    fireEvent.click(screen.getByTestId("child-count-pills"));
    expect(onClick).toHaveBeenCalledTimes(1);
    expect(rowClick).not.toHaveBeenCalled();
  });
});

describe("OrchestrationTab with a stalled child (#783)", () => {
  it("counts a live stalled child as stale on the node's pills", () => {
    const children = [child("a", { stalled: true }), child("b"), child("c", { status: "failed" })];
    const counts = countChildren(children);
    expect(counts).toEqual({ finished: 0, failed: 1, stale: 1, running: 1 });
    render(
      <>
        <ChildCountPills counts={counts} size="xs" testId="tab-child-pills" />
        <OrchestrationTab childRuns={children} counts={counts} nodeLive nodeAwaiting={false} onOpenChild={() => {}} />
      </>,
    );
    expect(screen.getByTestId("tab-child-pills-stale")).toHaveTextContent("1");
    expect(screen.getAllByTestId("orchestration-child")).toHaveLength(3);
  });
});

describe("OrchestrationTab with an awaiting child (#588)", () => {
  it("tints the awaiting child's row amber and signposts it", () => {
    const children = [child("a", { status: "awaiting_user" }), child("b")];
    render(
      <OrchestrationTab
        childRuns={children}
        counts={countChildren(children)}
        nodeLive
        nodeAwaiting
        onOpenChild={() => {}}
      />,
    );
    const rows = screen.getAllByTestId("orchestration-child");
    expect(rows[0]).toHaveAttribute("data-awaiting", "true");
    expect(rows[0].className).toContain("border-st-await");
    expect(screen.getByTestId("orchestration-child-awaiting")).toHaveTextContent("awaiting you");
    expect(rows[1]).not.toHaveAttribute("data-awaiting");
    // An awaiting child stays in the running bucket (no awaiting pill).
    expect(countChildren(children)).toEqual({ finished: 0, failed: 0, stale: 0, running: 2 });
  });
});
