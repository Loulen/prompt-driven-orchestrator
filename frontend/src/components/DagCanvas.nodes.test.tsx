import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { ReactFlowProvider } from "@xyflow/react";
import type { NodeProps, Node } from "@xyflow/react";
import { StartNode, EndNode } from "./DagCanvas";
import type { PortBrief } from "../types";

function Wrapper({ children }: { children: React.ReactNode }) {
  return <ReactFlowProvider>{children}</ReactFlowProvider>;
}

const ACC_GREEN = "var(--color-acc, #10b981)";
const BLOCKED_ORANGE = "var(--color-st-blocked, #f97316)";
const DONE_BG = "var(--color-st-done-bg, rgba(16,185,129,0.14))";
const NEUTRAL_BG = "var(--color-bg-3, #1e1f23)";

function nodeProps<T extends Record<string, unknown>>(
  id: string,
  type: string,
  data: T,
): NodeProps<Node<T>> {
  return {
    id,
    data,
    type,
    selected: false,
    isConnectable: true,
    zIndex: 0,
    positionAbsoluteX: 0,
    positionAbsoluteY: 0,
    dragging: false,
    deletable: true,
    selectable: true,
    parentId: undefined,
    dragHandle: undefined,
    sourcePosition: undefined,
    targetPosition: undefined,
    width: 32,
    height: 32,
  } as unknown as NodeProps<Node<T>>;
}

const START_OUTPUTS: PortBrief[] = [{ name: "user_prompt", side: "right" }];
const END_INPUTS: PortBrief[] = [{ name: "result", side: "left" }];

function startProps(reached: boolean) {
  return nodeProps("start", "start", { outputs: START_OUTPUTS, reached });
}

function endProps(reached: boolean) {
  return nodeProps("end", "end", { inputs: END_INPUTS, reached });
}

describe("EndNode (issue #105)", () => {
  it("is orange/blocked while the run has not reached the end", () => {
    render(<EndNode {...endProps(false)} />, { wrapper: Wrapper });
    const el = screen.getByTestId("node-end");
    expect(el.style.borderColor).toBe(BLOCKED_ORANGE);
    expect(el.style.color).toBe(BLOCKED_ORANGE);
    expect(el.style.background).toBe(NEUTRAL_BG);
    expect(el.getAttribute("data-reached")).toBe("false");
  });

  it("turns green with a done-tinted background once the end is reached", () => {
    render(<EndNode {...endProps(true)} />, { wrapper: Wrapper });
    const el = screen.getByTestId("node-end");
    expect(el.style.borderColor).toBe(ACC_GREEN);
    expect(el.style.color).toBe(ACC_GREEN);
    expect(el.style.background).toBe(DONE_BG);
    expect(el.getAttribute("data-reached")).toBe("true");
  });

  it("still renders its structural icon and input port", () => {
    render(<EndNode {...endProps(true)} />, { wrapper: Wrapper });
    expect(screen.getByTestId("node-icon-end")).toBeInTheDocument();
  });
});

describe("StartNode (issue #105)", () => {
  it("keeps its green outline but a neutral background before completion", () => {
    render(<StartNode {...startProps(false)} />, { wrapper: Wrapper });
    const el = screen.getByTestId("node-start");
    expect(el.style.borderColor).toBe(ACC_GREEN);
    expect(el.style.color).toBe(ACC_GREEN);
    expect(el.style.background).toBe(NEUTRAL_BG);
    expect(el.getAttribute("data-reached")).toBe("false");
  });

  it("picks up the green done-tinted background once the run reaches the end (iso with end)", () => {
    render(<StartNode {...startProps(true)} />, { wrapper: Wrapper });
    const el = screen.getByTestId("node-start");
    expect(el.style.borderColor).toBe(ACC_GREEN);
    expect(el.style.background).toBe(DONE_BG);
    expect(el.getAttribute("data-reached")).toBe("true");
  });

  it("still renders its structural icon", () => {
    render(<StartNode {...startProps(false)} />, { wrapper: Wrapper });
    expect(screen.getByTestId("node-icon-start")).toBeInTheDocument();
  });
});
