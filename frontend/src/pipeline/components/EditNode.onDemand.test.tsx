import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { ReactFlowProvider } from "@xyflow/react";
import { TooltipProvider } from "../../shared/ui/tooltip";
import { EditNode } from "./EditNode";
import { deriveEditNodes } from "./editNodeDerivation";
import type { NodeStatus } from "../../shared/generated/NodeStatus";
import type { RunState } from "../../shared/generated/RunState";
import type { EditorPipelineDef } from "../model/pipeline";

function Wrapper({ children }: { children: React.ReactNode }) {
  return (
    <TooltipProvider>
      <ReactFlowProvider>{children}</ReactFlowProvider>
    </TooltipProvider>
  );
}

function card(data: {
  status?: NodeStatus;
  onDemand?: boolean;
  inRun?: boolean;
  play?: "start" | "retry";
  onPlay?: () => void;
}) {
  return {
    id: "review",
    type: "edit",
    selected: false,
    isConnectable: false,
    zIndex: 0,
    positionAbsoluteX: 0,
    positionAbsoluteY: 0,
    dragging: false,
    deletable: true,
    selectable: true,
    width: 160,
    height: 80,
    data: {
      label: "Review",
      nodeId: "review",
      nodeType: "agent",
      status: data.status ?? "pending",
      inputs: [],
      outputs: [{ name: "out", side: "right" }],
      interactive: false,
      onDemand: data.onDemand,
      inRun: data.inRun,
      play: data.play,
      onPlay: data.onPlay,
    },
  } as unknown as Parameters<typeof EditNode>[0];
}

describe("the card of an on-demand node (ADR-0086)", () => {
  it("is greyed with a ▶ in a Run that never launched it, and the ▶ starts it", () => {
    const onPlay = vi.fn();
    render(<EditNode {...card({ onDemand: true, inRun: true, play: "start", onPlay })} />, {
      wrapper: Wrapper,
    });
    const node = screen.getByTestId("node-card");
    expect(node.style.opacity).toBe("0.6");
    expect(node.style.borderStyle).toBe("dashed");
    expect(node.style.borderColor).toBe("var(--color-fg-2)");
    const play = screen.getByTestId("node-play-btn");
    expect(play.dataset.play).toBe("start");
    fireEvent.click(play);
    expect(onPlay).toHaveBeenCalledOnce();
  });

  it("has no ▶ when the canvas grants none (archived Run, no right)", () => {
    render(<EditNode {...card({ onDemand: true, inRun: true })} />, { wrapper: Wrapper });
    expect(screen.getByTestId("node-card").style.opacity).toBe("0.6");
    expect(screen.queryByTestId("node-play-btn")).toBeNull();
  });

  it("is drawn like any node once launched", () => {
    render(<EditNode {...card({ onDemand: true, inRun: true, status: "running" })} />, {
      wrapper: Wrapper,
    });
    expect(screen.getByTestId("node-card").style.opacity).toBe("");
    expect(screen.queryByTestId("node-play-btn")).toBeNull();
  });

  it("carries a discreet marker on a template, not the greyed look", () => {
    render(<EditNode {...card({ onDemand: true, inRun: false })} />, { wrapper: Wrapper });
    expect(screen.getByTestId("on-demand-marker")).toBeTruthy();
    expect(screen.getByTestId("node-card").style.opacity).toBe("");
  });

  it("a skipped node carries a ▶ that retries it", () => {
    const onPlay = vi.fn();
    render(<EditNode {...card({ status: "skipped", inRun: true, play: "retry", onPlay })} />, {
      wrapper: Wrapper,
    });
    const play = screen.getByTestId("node-play-btn");
    expect(play.dataset.play).toBe("retry");
    fireEvent.click(play);
    expect(onPlay).toHaveBeenCalledOnce();
  });
});

describe("deriveEditNodes and « on demand » (ADR-0086)", () => {
  const pipeline: EditorPipelineDef = {
    name: "p",
    version: "1.0",
    variables: {},
    nodes: [
      { id: "review", name: "Review", type: "agent", interactive: false, inputs: [], outputs: [], on_demand: true },
      { id: "worker", name: "Worker", type: "agent", interactive: false, inputs: [], outputs: [] },
    ],
    edges: [],
  };

  it("passes the option and whether the card is drawn for a Run", () => {
    const template = deriveEditNodes(pipeline, null);
    expect(template.map((n) => [n.id, n.data.onDemand, n.data.inRun])).toEqual([
      ["review", true, false],
      ["worker", false, false],
    ]);
    const run = { run_id: "r1", status: "completed", nodes: {}, node_defs: [] } as unknown as RunState;
    const inRun = deriveEditNodes(pipeline, run);
    expect(inRun[0].data.inRun).toBe(true);
    expect(inRun[0].data.status).toBe("pending");
  });
});
