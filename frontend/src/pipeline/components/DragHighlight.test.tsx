import { render } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { ReactFlowProvider } from "@xyflow/react";
import { DragHighlightProvider } from "./DragHighlightContext";
import PortPill from "./PortPill";

function harness(dragHighlightNodeId: string | null) {
  return render(
    <ReactFlowProvider>
      <DragHighlightProvider value={dragHighlightNodeId}>
        {/* Node A: one input, one output */}
        <div data-testid="node-a">
          <PortPill
            id="in"
            kind="input"
            side="left"
            label="in"
            index={0}
            total={1}
            isDrop={dragHighlightNodeId === "a"}
          />
          <PortPill
            id="out"
            kind="output"
            side="right"
            label="out"
            index={0}
            total={1}
          />
        </div>
        {/* Node B: one input, one output */}
        <div data-testid="node-b">
          <PortPill
            id="in-b"
            kind="input"
            side="left"
            label="in"
            index={0}
            total={1}
            isDrop={dragHighlightNodeId === "b"}
          />
          <PortPill
            id="out-b"
            kind="output"
            side="right"
            label="out"
            index={0}
            total={1}
          />
        </div>
      </DragHighlightProvider>
    </ReactFlowProvider>,
  );
}

describe("DragHighlight", () => {
  it("no ports highlight when drag is inactive (null context)", () => {
    const { container } = harness(null);
    const drops = container.querySelectorAll(".port-pill.is-drop");
    expect(drops).toHaveLength(0);
  });

  it("hovering node A highlights only its input port", () => {
    const { container } = harness("a");
    const nodeA = container.querySelector("[data-testid='node-a']")!;
    const nodeB = container.querySelector("[data-testid='node-b']")!;

    const aInputs = nodeA.querySelectorAll(".port-pill.kind-input.is-drop");
    expect(aInputs).toHaveLength(1);

    const aOutputs = nodeA.querySelectorAll(".port-pill.kind-output.is-drop");
    expect(aOutputs).toHaveLength(0);

    const bDrops = nodeB.querySelectorAll(".port-pill.is-drop");
    expect(bDrops).toHaveLength(0);
  });

  it("output ports of the hovered node do not light up", () => {
    const { container } = harness("a");
    const outputs = container.querySelectorAll(".port-pill.kind-output.is-drop");
    expect(outputs).toHaveLength(0);
  });

  it("leaving node A and entering node B transfers the highlight", () => {
    const { container: c1 } = harness("a");
    expect(c1.querySelector("[data-testid='node-a'] .port-pill.is-drop")).toBeTruthy();
    expect(c1.querySelector("[data-testid='node-b'] .port-pill.is-drop")).toBeFalsy();

    const { container: c2 } = harness("b");
    expect(c2.querySelector("[data-testid='node-a'] .port-pill.is-drop")).toBeFalsy();
    expect(c2.querySelector("[data-testid='node-b'] .port-pill.is-drop")).toBeTruthy();
  });

  it("full sequence: drag start → hover A → leave A → hover B → drag end", () => {
    // Step 1: drag starts, no node hovered → no highlights
    const { container: s1 } = harness(null);
    expect(s1.querySelectorAll(".is-drop")).toHaveLength(0);

    // Step 2: cursor enters node A → A's input highlighted
    const { container: s2 } = harness("a");
    expect(s2.querySelector("[data-testid='node-a'] .port-pill.kind-input.is-drop")).toBeTruthy();
    expect(s2.querySelector("[data-testid='node-b'] .port-pill.is-drop")).toBeFalsy();

    // Step 3: cursor leaves A → no highlights
    const { container: s3 } = harness(null);
    expect(s3.querySelectorAll(".is-drop")).toHaveLength(0);

    // Step 4: cursor enters node B → B's input highlighted, A clean
    const { container: s4 } = harness("b");
    expect(s4.querySelector("[data-testid='node-a'] .port-pill.is-drop")).toBeFalsy();
    expect(s4.querySelector("[data-testid='node-b'] .port-pill.kind-input.is-drop")).toBeTruthy();

    // Step 5: drag ends → no highlights
    const { container: s5 } = harness(null);
    expect(s5.querySelectorAll(".is-drop")).toHaveLength(0);
  });
});
