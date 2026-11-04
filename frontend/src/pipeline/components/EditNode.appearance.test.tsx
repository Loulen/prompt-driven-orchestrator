/**
 * A node's card on a Run canvas with a given Apparence (CONTEXT.md « Élément
 * masquable »): an element hidden is not drawn; the type icon, the name, the
 * status and an orchestrator's child runs always are; « Compact nodes » has
 * the last word; the editor, and a Run edited for itself, show every element.
 */
import { render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { ReactFlowProvider } from "@xyflow/react";
import { resetAppearance } from "../../hooks/useAppearance";
import { COMPACT_NODES_STORAGE_KEY } from "../../lib/compactNodes";
import { ELEMENTS_STORAGE_KEY } from "../../lib/appearance";
import { EditNode, type EditNodeData } from "./EditNode";

const RUN = {
  iter: 2,
  iterations: 2,
  startedAt: "2026-01-01T10:00:00Z",
  completedAt: "2026-01-01T10:07:00Z",
  cost: { usd: 1.24, form: "reported" as const, reported_in_usd: true, partial: false, executions: 1, readable_executions: 1 },
};

const DATA: Partial<EditNodeData> = {
  inRun: true,
  status: "completed",
  run: RUN,
  agent: { short: "Opus", title: "Agent" },
  isolated: true,
  agentMode: "profile",
  loopBadge: { text: "↻ 2/3", kind: "bounded" },
  orchestrator: true,
  childCounts: { running: 1, failed: 0, stale: 0, finished: 2 },
};

function draw(data: Partial<EditNodeData>) {
  const props = {
    id: "tdd",
    type: "edit",
    selected: false,
    isConnectable: false,
    zIndex: 0,
    positionAbsoluteX: 0,
    positionAbsoluteY: 0,
    dragging: false,
    deletable: true,
    selectable: true,
    data: { label: "tdd", nodeId: "tdd", nodeType: "agent", inputs: [], outputs: [{ name: "out" }], interactive: false, ...data },
  } as unknown as Parameters<typeof EditNode>[0];
  return render(
    <ReactFlowProvider>
      <EditNode {...props} />
    </ReactFlowProvider>,
  );
}

/** This browser's copy of the Apparence holds `elements` and « Compact nodes », read at the next render. */
function appearance(elements: Record<string, boolean>, compactNodes = false) {
  localStorage.setItem(ELEMENTS_STORAGE_KEY, JSON.stringify(elements));
  localStorage.setItem(COMPACT_NODES_STORAGE_KEY, String(compactNodes));
  resetAppearance();
}

const cells = () => [...document.querySelectorAll(".cv-n__cell")].map((cell) => cell.getAttribute("title")?.split(":")[0]);

afterEach(() => {
  localStorage.clear();
  resetAppearance();
});

describe("a node's card on a Run canvas, with an Apparence", () => {
  it("shows the defaults: Elapsed and Cost on one row, the loop badge and the profile marker, no model, iteration or isolation flag", () => {
    appearance({});
    draw({ ...DATA, runCanvas: true });
    expect(cells()).toEqual(["Elapsed", "Cost"]);
    expect(screen.getByTestId("loop-badge")).toBeInTheDocument();
    expect(screen.getByTestId("agent-mode-profile")).toBeInTheDocument();
    expect(screen.queryByTestId("isolation-marker")).toBeNull();
  });

  it("leaves out each hidden element, and keeps the icon, the name, the status and the child runs", () => {
    appearance({
      "run.canvas.elapsed": false,
      "run.canvas.cost": false,
      "run.canvas.loop": false,
      "run.canvas.profile": false,
    });
    draw({ ...DATA, runCanvas: true });
    // No cell left: no grid, the card is its header (and its runs).
    expect(document.querySelector(".cv-n__grid")).toBeNull();
    expect(screen.queryByTestId("loop-badge")).toBeNull();
    expect(screen.queryByTestId("agent-mode-profile")).toBeNull();
    const head = screen.getByTestId("node-card-head");
    expect(screen.getByTestId("node-icon-agent")).toBeInTheDocument();
    expect(within(head).getByText("tdd")).toBeInTheDocument();
    expect(within(head).getByRole("img", { name: "Completed" })).toBeInTheDocument();
    expect(screen.getByTestId("node-child-runs")).toHaveTextContent("3 runs");
  });

  it("shows what the user turns on: the model, the iteration and the isolation flag", () => {
    appearance({ "run.canvas.agent": true, "run.canvas.iteration": true, "run.canvas.isolation": true });
    draw({ ...DATA, runCanvas: true });
    expect(cells()).toEqual(["Agent", "Iteration", "Elapsed", "Cost"]);
    expect(screen.getByTestId("isolation-marker")).toBeInTheDocument();
  });

  it("keeps a missing agent profile, an alert, whatever the Apparence", () => {
    appearance({ "run.canvas.profile": false });
    draw({ ...DATA, agentMode: "broken", runCanvas: true });
    expect(screen.getByTestId("agent-mode-broken")).toBeInTheDocument();
  });

  it("gives « Compact nodes » the last word: one line, the icon, the name, the status and the runs, whatever the switches", () => {
    appearance({ "run.canvas.agent": true, "run.canvas.isolation": true }, true);
    draw({ ...DATA, runCanvas: true, compact: true });
    expect(screen.getByTestId("node-card")).toHaveAttribute("data-compact", "true");
    expect(document.querySelector(".cv-n__grid")).toBeNull();
    expect(screen.queryByTestId("isolation-marker")).toBeNull();
    expect(screen.queryByTestId("loop-badge")).toBeNull();
    expect(screen.getByTestId("node-child-runs")).toBeInTheDocument();
  });

  it("shows every element in the editor and on a Run edited for itself (no `runCanvas`), whatever the Apparence", () => {
    appearance({ "run.canvas.elapsed": false, "run.canvas.cost": false, "run.canvas.loop": false, "run.canvas.profile": false }, true);
    draw(DATA);
    expect(cells()).toEqual(["Agent", "Iteration", "Elapsed", "Cost"]);
    expect(screen.getByTestId("isolation-marker")).toBeInTheDocument();
    expect(screen.getByTestId("loop-badge")).toBeInTheDocument();
    expect(screen.getByTestId("agent-mode-profile")).toBeInTheDocument();
    expect(screen.getByTestId("node-card")).not.toHaveAttribute("data-compact");
  });

  it("keeps a template's card whole: what runs the node, its worktree, its outputs", () => {
    appearance({ "run.canvas.agent": false });
    draw({ agent: { short: "Opus", title: "Agent" } });
    expect(cells()).toEqual(["Agent", "Worktree", "out · markdown"]);
  });
});
