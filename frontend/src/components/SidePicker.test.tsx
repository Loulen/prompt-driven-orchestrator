import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import SidePicker from "./SidePicker";
import NodeInspector from "./NodeInspector";
import { TooltipProvider } from "./ui/tooltip";
import { useEditStore } from "../stores/editStore";
import type { NodeDef, PipelineDef } from "../types";

vi.mock("../api", () => ({
  fetchAgentProfiles: vi.fn().mockResolvedValue({ profiles: [] }),
  // #669: the skills selector's reads (bank + inherited tiers), empty by default.
  fetchSkillBank: vi.fn().mockResolvedValue({ skills: [], folders: [], root_path: "" }),
  fetchProjects: vi.fn().mockResolvedValue([]),
  saveToLibrary: vi.fn(),
  deleteFromLibrary: vi.fn(),
  instantiateFromLibrary: vi.fn(),
  // #586: NodeInspector's harness picker fetches /settings for its options.
  fetchSettings: vi.fn().mockResolvedValue({ harness_descriptors: null }),
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
      type: "agent",
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
        { id: "tab1", scope: "repo", pipeline, prompts: {}, diagnostics: [], dirty: false, externalDirty: false },
      ],
      activeTabId: "tab1",
      selection: { kind: "node", id: "n1" },
    });
  });

  // Inputs are emergent (#149): they are read-only and carry no side picker.
  // The side picker now lives only on the declared OUTPUT port rows.
  it("renders SidePicker buttons (L/R/T/B) on output port rows", () => {
    render(<TooltipProvider><NodeInspector libraryEntries={[]} onLibraryChanged={() => {}} /></TooltipProvider>);
    expect(screen.getAllByTitle("left").length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByTitle("right").length).toBeGreaterThanOrEqual(1);
  });

  it("clicking a side button updates the output port side", () => {
    render(<TooltipProvider><NodeInspector libraryEntries={[]} onLibraryChanged={() => {}} /></TooltipProvider>);
    const topButtons = screen.getAllByTitle("top");
    fireEvent.click(topButtons[0]);

    const state = useEditStore.getState();
    const updatedNode = state.openTabs[0].pipeline.nodes.find((n) => n.id === "n1")!;
    expect(updatedNode.outputs[0].side).toBe("top");
  });
});
