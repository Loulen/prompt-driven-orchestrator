import { render, act } from "@testing-library/react";
import { describe, it, expect, afterEach } from "vitest";
import { ReactFlowProvider } from "@xyflow/react";
import OrthogonalEdge, { type OrthogonalEdgeData } from "./OrthogonalEdge";
import { useEditStore } from "../stores/editStore";

// OrthogonalEdge reads the selection from the global edit store (the same
// source of truth the edge detail panel keys off). Reset it between tests so an
// edge is never accidentally rendered "selected".
afterEach(() => {
  useEditStore.getState().setSelection({ kind: "none", id: null });
});

function Wrapper({ children }: { children: React.ReactNode }) {
  return <ReactFlowProvider>{children}</ReactFlowProvider>;
}

function edgeProps(edgeIndex: number, data?: Partial<OrthogonalEdgeData>) {
  return {
    id: `e-${edgeIndex}`,
    source: "a",
    target: "b",
    sourceX: 0,
    sourceY: 0,
    targetX: 100,
    targetY: 0,
    markerEnd: "",
    data: {
      edgeIndex,
      mode: null,
      waypoints: null,
      isConditional: false,
      isElse: false,
      // Derivation now hands every edge the grey default; selection recolors it.
      strokeColor: "var(--color-fg-4)",
      dashed: false,
      ...data,
    },
  } as unknown as Parameters<typeof OrthogonalEdge>[0];
}

// The visible stroke lives on xyflow's BaseEdge `<path class="react-flow__edge-path">`.
function edgeStroke(container: HTMLElement): string {
  const path = container.querySelector<SVGPathElement>(".react-flow__edge-path");
  if (!path) throw new Error("edge path not found");
  return path.style.stroke;
}

describe("OrthogonalEdge selection color (#177)", () => {
  it("renders grey when no edge is selected", () => {
    const { container } = render(<OrthogonalEdge {...edgeProps(0)} />, { wrapper: Wrapper });
    expect(edgeStroke(container)).toContain("--color-fg-4");
    expect(edgeStroke(container)).not.toContain("--color-edge-selected");
  });

  it("turns pastel orange when this edge is the selected one", () => {
    const { container } = render(<OrthogonalEdge {...edgeProps(0)} />, { wrapper: Wrapper });
    act(() => {
      useEditStore.getState().setSelection({ kind: "edge", id: null, edgeIndex: 0 });
    });
    expect(edgeStroke(container)).toContain("--color-edge-selected");
  });

  it("restores grey when the edge is deselected", () => {
    const { container } = render(<OrthogonalEdge {...edgeProps(0)} />, { wrapper: Wrapper });
    act(() => {
      useEditStore.getState().setSelection({ kind: "edge", id: null, edgeIndex: 0 });
    });
    expect(edgeStroke(container)).toContain("--color-edge-selected");
    act(() => {
      useEditStore.getState().setSelection({ kind: "none", id: null });
    });
    expect(edgeStroke(container)).toContain("--color-fg-4");
    expect(edgeStroke(container)).not.toContain("--color-edge-selected");
  });

  it("only the selected edge is orange — a different selected index leaves this one grey", () => {
    // The store holds at most one selection, so at most one edge is ever orange.
    const { container } = render(<OrthogonalEdge {...edgeProps(0)} />, { wrapper: Wrapper });
    act(() => {
      useEditStore.getState().setSelection({ kind: "edge", id: null, edgeIndex: 3 });
    });
    expect(edgeStroke(container)).toContain("--color-fg-4");
    expect(edgeStroke(container)).not.toContain("--color-edge-selected");
  });

  it("a selected conditional/end edge is orange too — selection overrides any base color", () => {
    const { container } = render(
      <OrthogonalEdge {...edgeProps(0, { isConditional: true, dashed: true })} />,
      { wrapper: Wrapper },
    );
    act(() => {
      useEditStore.getState().setSelection({ kind: "edge", id: null, edgeIndex: 0 });
    });
    expect(edgeStroke(container)).toContain("--color-edge-selected");
  });
});
