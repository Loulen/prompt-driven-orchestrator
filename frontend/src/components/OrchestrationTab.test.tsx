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
    // Each counter wears the Status icon of what it counts: stale = the list's stalled glyph.
    expect(screen.getByTestId("child-count-pills-finished").querySelector('[data-status="completed"]')).not.toBeNull();
    expect(screen.getByTestId("child-count-pills-failed").querySelector('[data-status="failed"]')).not.toBeNull();
    expect(screen.getByTestId("child-count-pills-stale").querySelector('[data-status="stalled"]')).not.toBeNull();
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

// PDO-22 (#999): each child wears the list's Status icon — the one table, so a
// halted / skipped / archived child never reads as completed, paused stays paused.
describe("OrchestrationTab child Status icons (#999)", () => {
  it("gives every child the glyph of its own status", () => {
    const children = [
      child("done", { status: "completed" }),
      child("halt", { status: "halted" }),
      child("skip", { status: "skipped" }),
      child("arch", { status: "archived" }),
      child("pause", { status: "paused" }),
      child("stall", { stalled: true }),
    ];
    render(
      <OrchestrationTab childRuns={children} counts={countChildren(children)} nodeLive={false} nodeAwaiting={false} onOpenChild={() => {}} />,
    );
    const icons = screen.getAllByTestId("orchestration-child-status");
    expect(icons.map((i) => i.getAttribute("data-status"))).toEqual([
      "completed",
      "halted",
      "skipped",
      "archived",
      "paused",
      "stalled",
    ]);
    // Named for assistive tech, with a tooltip — no text label sits beside it.
    expect(icons[1]).toHaveAttribute("role", "img");
    expect(icons[1]).toHaveAccessibleName("Halted");
    expect(icons[1]).toHaveAttribute("title", "Halted");
    expect(icons[4]).toHaveAccessibleName("Paused");
    expect(icons.filter((i) => i.getAttribute("data-status") === "completed")).toHaveLength(1);
  });

  it("puts the child's reason in the tooltip, like the run list and Run info", () => {
    const children = [
      child("fail", { status: "failed", failure_reason: "exit 1" }),
      child("halt", { status: "halted", failure_reason: "Halted: verdict was FAIL" }),
      child("wait", { status: "awaiting_user", awaiting_reason: "which repo?" }),
    ];
    render(
      <OrchestrationTab childRuns={children} counts={countChildren(children)} nodeLive={false} nodeAwaiting={false} onOpenChild={() => {}} />,
    );
    const icons = screen.getAllByTestId("orchestration-child-status");
    expect(icons.map((i) => i.getAttribute("title"))).toEqual([
      "Failed: exit 1",
      "Halted: verdict was FAIL",
      "Awaiting you: which repo?",
    ]);
    // The accessible name stays the bare label.
    expect(icons[0]).toHaveAccessibleName("Failed");
  });
});

// pdo-issues#3 (ADR-0081): each child shows its tree cost; a child that has
// descendants wears a « ⑂ N » badge whose tooltip splits it; Σ sums tree costs.
describe("OrchestrationTab tree cost", () => {
  const claude = (usd: number, partial = false) => ({
    usd,
    partial,
    unpriced_models: partial ? ["mystery"] : [],
    by_harness: [
      { harness: "claude", usd, form: "derived" as const, partial, unpriced_models: partial ? ["mystery"] : [] },
    ],
  });
  const orchestrating = child("a", {
    status: "completed",
    cost: claude(2.41),
    tree_cost: {
      total: claude(3.76),
      parent: claude(2.41),
      children: claude(1.35),
      descendants: 1,
      by_depth: [{ depth: 1, runs: 1, cost: claude(1.35) }],
      unavailable_runs: 0,
    },
  });
  const leaf = child("b", { status: "completed", cost: claude(1.21) });

  it("shows the child's tree cost, a fork badge only on a child with descendants, and Σ of tree costs", () => {
    const children = [orchestrating, leaf];
    render(
      <OrchestrationTab
        childRuns={children}
        counts={countChildren(children)}
        nodeLive={false}
        nodeAwaiting={false}
        onOpenChild={() => {}}
      />,
    );
    const rows = screen.getAllByTestId("orchestration-child");
    expect(rows[0].querySelector('[data-testid="orchestration-child-cost"]')).toHaveTextContent("~$3.76");
    expect(rows[0].querySelector('[data-testid="orchestration-child-tree-badge"]')).toHaveTextContent("1");
    expect(rows[1].querySelector('[data-testid="orchestration-child-cost"]')).toHaveTextContent("~$1.21");
    expect(rows[1].querySelector('[data-testid="orchestration-child-tree-badge"]')).toBeNull();
    const sum = screen.getByTestId("orchestration-sum");
    expect(sum).toHaveTextContent("Σ ~$4.97");
    expect(sum).toHaveAttribute("title", "Sum of the children's tree costs (grandchildren included)");
  });

  it("splits the tree cost into Parent run / Child runs (N) in the badge's tooltip", async () => {
    render(
      <OrchestrationTab
        childRuns={[orchestrating]}
        counts={countChildren([orchestrating])}
        nodeLive={false}
        nodeAwaiting={false}
        onOpenChild={() => {}}
      />,
    );
    const badge = screen.getByTestId("orchestration-child-tree-badge");
    expect(badge).toHaveAttribute("aria-label", "Tree cost: parent run ~$2.41, 1 child run ~$1.35");
    fireEvent.pointerMove(badge);
    fireEvent.focus(badge);
    const tip = await screen.findAllByTestId("orchestration-child-tree-tooltip");
    expect(tip[0]).toHaveTextContent("Parent run");
    expect(tip[0]).toHaveTextContent("~$2.41");
    expect(tip[0]).toHaveTextContent("Child runs (1)");
    expect(tip[0]).toHaveTextContent("~$1.35");
    expect(tip[0]).toHaveTextContent("Tree cost: this child run and its descendants.");
  });

  it("renders '—' for a child whose tree total is unavailable, and Σ marks itself partial", () => {
    const broken = child("c", {
      status: "completed",
      cost: claude(1),
      tree_cost: {
        total: null,
        parent: claude(1),
        children: null,
        descendants: 1,
        by_depth: [{ depth: 1, runs: 1, cost: null }],
        unavailable_runs: 1,
      },
    });
    const children = [broken, leaf];
    render(
      <OrchestrationTab
        childRuns={children}
        counts={countChildren(children)}
        nodeLive={false}
        nodeAwaiting={false}
        onOpenChild={() => {}}
      />,
    );
    const rows = screen.getAllByTestId("orchestration-child");
    expect(rows[0].querySelector('[data-testid="orchestration-child-cost"]')).toHaveTextContent("—");
    expect(screen.getByTestId("orchestration-sum")).toHaveTextContent("Σ ~$1.21†");
  });
});
