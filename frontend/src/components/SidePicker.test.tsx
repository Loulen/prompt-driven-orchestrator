import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import SidePicker from "./SidePicker";
import NodeInspector from "./NodeInspector";
import { useEditStore } from "../stores/editStore";
import type { NodeDef, PipelineDef } from "../types";

vi.mock("../api", () => ({
  saveToLibrary: vi.fn(),
  deleteFromLibrary: vi.fn(),
  instantiateFromLibrary: vi.fn(),
}));

describe("SidePicker", () => {
  it("renders all four sides", () => {
    render(<SidePicker value="left" onChange={() => {}} />);
    expect(screen.getByTitle("left")).toBeInTheDocument();
    expect(screen.getByTitle("right")).toBeInTheDocument();
    expect(screen.getByTitle("top")).toBeInTheDocument();
    expect(screen.getByTitle("bottom")).toBeInTheDocument();
  });

  it("highlights the active side", () => {
    render(<SidePicker value="right" onChange={() => {}} />);
    const rightBtn = screen.getByTitle("right");
    expect(rightBtn.className).toContain("bg-acc-bg");
    const leftBtn = screen.getByTitle("left");
    expect(leftBtn.className).not.toContain("bg-acc-bg");
  });

  it("calls onChange with the clicked side", () => {
    const onChange = vi.fn();
    render(<SidePicker value="left" onChange={onChange} />);
    fireEvent.click(screen.getByTitle("bottom"));
    expect(onChange).toHaveBeenCalledWith("bottom");
  });

  it("displays abbreviated labels", () => {
    render(<SidePicker value="left" onChange={() => {}} />);
    expect(screen.getByTitle("left")).toHaveTextContent("L");
    expect(screen.getByTitle("right")).toHaveTextContent("R");
    expect(screen.getByTitle("top")).toHaveTextContent("T");
    expect(screen.getByTitle("bottom")).toHaveTextContent("B");
  });
});

describe("SidePicker retrofit in NodeInspector PortRow", () => {
  function makeNode(): NodeDef {
    return {
      id: "n1",
      name: "test-node",
      type: "doc-only",
      inputs: [{ name: "in", repeated: false, side: "left" }],
      outputs: [{ name: "out", repeated: false, side: "right" }],
      interactive: false,
    };
  }

  function makePipeline(node: NodeDef): PipelineDef {
    return { name: "test", variables: {}, nodes: [node], edges: [] };
  }

  beforeEach(() => {
    Object.assign(navigator, {
      clipboard: { writeText: vi.fn().mockResolvedValue(undefined) },
    });

    const node = makeNode();
    const pipeline = makePipeline(node);
    useEditStore.setState({
      openTabs: [
        { id: "tab1", scope: "repo", pipeline, prompts: {}, dirty: false, externalDirty: false },
      ],
      activeTabId: "tab1",
      selection: { kind: "node", id: "n1" },
    });
  });

  it("renders SidePicker buttons (L/R/T/B) in port rows", () => {
    render(<NodeInspector libraryEntries={[]} onLibraryChanged={() => {}} />);
    const sideButtons = screen.getAllByTitle("left");
    expect(sideButtons.length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByTitle("right").length).toBeGreaterThanOrEqual(1);
  });

  it("clicking a side button updates the port side", () => {
    render(<NodeInspector libraryEntries={[]} onLibraryChanged={() => {}} />);
    const topButtons = screen.getAllByTitle("top");
    fireEvent.click(topButtons[0]);

    const state = useEditStore.getState();
    const updatedNode = state.openTabs[0].pipeline.nodes.find((n) => n.id === "n1")!;
    expect(updatedNode.inputs[0].side).toBe("top");
  });
});
