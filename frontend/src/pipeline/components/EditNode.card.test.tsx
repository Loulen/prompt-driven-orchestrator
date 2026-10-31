import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ReactFlowProvider } from "@xyflow/react";
import { EditNode, type EditNodeData } from "./EditNode";
import type { ChildCounts } from "../../run/model/orchestration";

function card(data: Partial<EditNodeData>) {
  return {
    id: "dispatch",
    type: "edit",
    selected: false,
    isConnectable: false,
    zIndex: 0,
    positionAbsoluteX: 0,
    positionAbsoluteY: 0,
    dragging: false,
    deletable: true,
    selectable: true,
    data: {
      label: "Dispatch fixes",
      nodeId: "dispatch",
      nodeType: "agent",
      status: "pending",
      inputs: [],
      outputs: [{ name: "out" }],
      interactive: false,
      ...data,
    },
  } as unknown as Parameters<typeof EditNode>[0];
}

function draw(data: Partial<EditNodeData>) {
  return render(
    <ReactFlowProvider>
      <EditNode {...card(data)} />
    </ReactFlowProvider>,
  );
}

const RUN = { iter: 1, iterations: 1, startedAt: null, completedAt: null, cost: null };

describe("the header of a node card (blueprint header)", () => {
  it("leads with the type icon, painted in the Theme's accent, on a template", () => {
    draw({});
    const head = screen.getByTestId("node-card-head");
    const icons = head.querySelectorAll("svg[role='img']");
    expect(icons[0]).toHaveAttribute("data-testid", "node-icon-agent");
    // `.cv-n__type` is the one place of the card in `--primary` (canvas.css).
    expect(icons[0]).toHaveClass("cv-n__type");
    // A template has no status: no status icon closes the header.
    expect(within(head).queryByRole("img", { name: "Completed" })).toBeNull();
  });

  it("leads with the type icon and closes on the status in a Run", () => {
    draw({ inRun: true, status: "completed", run: RUN });
    const head = screen.getByTestId("node-card-head");
    const icons = [...head.querySelectorAll("svg[role='img']")];
    expect(icons[0]).toHaveClass("cv-n__type");
    expect(icons.at(-1)).toHaveAccessibleName("Completed");
    expect(head.lastElementChild).toBe(icons.at(-1));
  });

  it("puts the flags between the name and the status", () => {
    draw({ inRun: true, status: "running", isolated: true, run: RUN });
    const head = screen.getByTestId("node-card-head");
    const order = [...head.children].map((c) => c.getAttribute("data-testid") ?? c.className.toString());
    const name = order.indexOf("cv-n__name");
    const flag = order.indexOf("isolation-marker");
    expect(name).toBeGreaterThan(-1);
    expect(flag).toBeGreaterThan(name);
  });

  it("names an orchestrator by its type icon, in the header and in the meta grid", () => {
    draw({ orchestrator: true });
    expect(screen.getAllByRole("img", { name: "Orchestrator" }).length).toBeGreaterThanOrEqual(1);
    expect(screen.getByTestId("node-icon-agent")).toHaveAccessibleName("Orchestrator");
  });
});

describe("the second sheet of an orchestrator", () => {
  it("marks the card of an orchestrator, on a template as in a Run", () => {
    const { unmount } = draw({ orchestrator: true });
    expect(screen.getByTestId("node-card")).toHaveAttribute("data-orch", "true");
    unmount();
    draw({ orchestrator: true, inRun: true, status: "running", run: RUN });
    expect(screen.getByTestId("node-card")).toHaveAttribute("data-orch", "true");
  });

  it("leaves any other card single", () => {
    draw({});
    expect(screen.getByTestId("node-card")).not.toHaveAttribute("data-orch");
  });
});

describe("an orchestrator's child runs on its card", () => {
  const orchestrator = (counts: ChildCounts, onOpenOrchestration?: () => void) =>
    draw({ orchestrator: true, inRun: true, status: "running", run: RUN, childCounts: counts, onOpenOrchestration });

  it("shows nothing while it has no child", () => {
    orchestrator({ finished: 0, failed: 0, stale: 0, running: 0 });
    expect(screen.queryByTestId("node-child-runs")).toBeNull();
  });

  it("says « 1 run » with its pastille for one child", () => {
    orchestrator({ finished: 0, failed: 0, stale: 0, running: 1 });
    const row = screen.getByTestId("node-child-runs");
    expect(row).toHaveTextContent(/^1 run1$/);
    expect(screen.getByTestId("node-child-pills-running")).toHaveTextContent("1");
  });

  it("counts 15 children of mixed statuses, one pastille per group, as on the Orchestration tab", () => {
    orchestrator({ finished: 7, failed: 2, stale: 0, running: 6 });
    expect(screen.getByTestId("node-child-runs")).toHaveTextContent(/^15 runs/);
    expect(screen.getByTestId("node-child-pills-finished")).toHaveTextContent("7");
    expect(screen.getByTestId("node-child-pills-failed")).toHaveTextContent("2");
    expect(screen.getByTestId("node-child-pills-running")).toHaveTextContent("6");
    expect(screen.queryByTestId("node-child-pills-stale")).toBeNull();
  });

  it("opens the Orchestration tab, without the click reaching the card", () => {
    const open = vi.fn();
    const cardClick = vi.fn();
    render(
      <ReactFlowProvider>
        <div onClick={cardClick}>
          <EditNode
            {...card({
              orchestrator: true,
              inRun: true,
              status: "running",
              run: RUN,
              childCounts: { finished: 1, failed: 0, stale: 0, running: 1 },
              onOpenOrchestration: open,
            })}
          />
        </div>
      </ReactFlowProvider>,
    );
    fireEvent.click(screen.getByTestId("node-child-runs"));
    expect(open).toHaveBeenCalledOnce();
    expect(cardClick).not.toHaveBeenCalled();
  });
});

describe("the cost on a Run's card", () => {
  it("shows the amount alone, no tokens", () => {
    draw({
      inRun: true,
      status: "completed",
      run: {
        ...RUN,
        cost: { usd: 0.05, partial: false, form: "reported", reported_in_usd: true } as never,
      },
    });
    const cost = screen.getByTitle(/^Cost: /);
    expect(cost).toHaveTextContent("$0.0500");
    expect(cost.textContent).not.toMatch(/tok/i);
  });
});
