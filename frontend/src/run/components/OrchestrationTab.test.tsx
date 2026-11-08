// #723/#783 — the Orchestration tab's shared pastilles: four pills (finished /
// failed / stale / running), zero pills hidden, the cluster absent without a child.
import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { ChildCountPills, OrchestrationTab } from "./OrchestrationTab";
import { countChildren, type RunChildEntry } from "../model/orchestration";

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
    expect(screen.getByTestId("child-count-pills-stale").querySelector(".bg-st-skipped")).not.toBeNull();
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

  it("lists the children under no line of explanation, live children included", () => {
    const children = [child("a"), child("b", { stalled: true })];
    render(
      <OrchestrationTab childRuns={children} counts={countChildren(children)} nodeLive nodeAwaiting={false} onOpenChild={() => {}} />,
    );
    expect(screen.queryByText(/completes when/i)).toBeNull();
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
    // Tinted in the awaiting tone (inspector.css `.ri-orch__row[data-st]`).
    expect(rows[0]).toHaveAttribute("data-st", "awaiting_user");
    expect(screen.getByTestId("orchestration-child-awaiting")).toHaveTextContent("awaiting you");
    expect(rows[1]).not.toHaveAttribute("data-awaiting");
    // An awaiting child stays in the running bucket (no awaiting pill).
    expect(countChildren(children)).toEqual({ finished: 0, failed: 0, stale: 0, running: 2 });
  });
});

describe("OrchestrationTab tree costs", () => {
  const usd = (amount: number) => ({ usd: amount, partial: false, unpriced_models: [], uncosted_harnesses: [], by_harness: [] });
  const childTree = {
    total: usd(2.3),
    parent: usd(1.3),
    children: usd(1),
    descendants: 1,
    by_depth: [{ depth: 1, runs: 1, cost: usd(1) }],
    unavailable_runs: 0,
  };

  it("shows a child's tree cost with a ⑂ N chip, and its own cost without one", () => {
    const children = [
      child("leaf", { status: "completed", cost: usd(1.1) }),
      child("parent", { status: "completed", cost: usd(1.3), tree_cost: childTree }),
    ];
    render(
      <OrchestrationTab childRuns={children} counts={countChildren(children)} nodeLive={false} nodeAwaiting={false} onOpenChild={() => {}} />,
    );
    const rows = screen.getAllByTestId("orchestration-child");
    expect(rows[0]).toHaveTextContent("~$1.10");
    expect(rows[0].querySelector("[data-testid=orchestration-child-tree-badge]")).toBeNull();
    expect(rows[1].querySelector("[data-testid=orchestration-child-cost]")).toHaveTextContent("~$2.30");
    const badge = rows[1].querySelector("[data-testid=orchestration-child-tree-badge]")!;
    expect(badge).toHaveTextContent("1");
    expect(badge.getAttribute("aria-label")).toContain("Parent run ~$1.30 · Child runs (1) ~$1.00");
    expect(badge.getAttribute("aria-label")).toContain("Tree cost: this child run and its descendants.");
    // Σ sums the children's tree costs, grandchildren included.
    const sum = screen.getByTestId("orchestration-sum");
    expect(sum).toHaveTextContent("Σ ~$3.40");
    expect(sum).toHaveAttribute("title", "Sum of the children's tree costs (grandchildren included)");
  });

  it("adds the Run's Tree cost line under Σ when the Run has a tree cost", () => {
    const children = [child("a", { status: "completed", cost: usd(3.4) })];
    const runTree = { ...childTree, total: usd(4.2), parent: usd(0.8), children: usd(3.4) };
    const { rerender } = render(
      <OrchestrationTab
        childRuns={children}
        counts={countChildren(children)}
        nodeLive={false}
        nodeAwaiting={false}
        onOpenChild={() => {}}
        runTreeCost={runTree}
      />,
    );
    const foot = screen.getByTestId("orchestration-tree-cost");
    expect(foot).toHaveTextContent("Tree cost · parent run ~$0.8000 + child runs ~$3.40");
    expect(screen.getByTestId("orchestration-tree-cost-total")).toHaveTextContent("~$4.20");
    rerender(
      <OrchestrationTab childRuns={children} counts={countChildren(children)} nodeLive={false} nodeAwaiting={false} onOpenChild={() => {}} />,
    );
    expect(screen.queryByTestId("orchestration-tree-cost")).toBeNull();
  });

  it("reads « — » for a child whose tree total is unavailable", () => {
    const children = [
      child("p", { status: "completed", cost: usd(1.3), tree_cost: { ...childTree, total: null, children: null, unavailable_runs: 1 } }),
    ];
    render(
      <OrchestrationTab childRuns={children} counts={countChildren(children)} nodeLive={false} nodeAwaiting={false} onOpenChild={() => {}} />,
    );
    expect(screen.getByTestId("orchestration-child-cost")).toHaveTextContent("—");
    expect(screen.getByTestId("orchestration-sum")).toHaveTextContent("Σ —");
  });
});
