import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, beforeEach } from "vitest";
import RegionInspector from "./RegionInspector";
import { useEditStore } from "../stores/editStore";
import type { PipelineDef, NodeDef, LoopRegion } from "../types";

function makeNode(id: string, name: string): NodeDef {
  return {
    id,
    name,
    type: "code-mutating",
    inputs: [],
    outputs: [{ name: "out", repeated: false, side: "right" }],
    interactive: false,
  };
}

function makePipeline(loops: LoopRegion[]): PipelineDef {
  return {
    name: "test-pipeline",
    variables: {},
    nodes: [makeNode("impl", "implementer"), makeNode("rev", "reviewer")],
    edges: [],
    loops,
  };
}

function selectRegion(loops: LoopRegion[], regionId: string) {
  useEditStore.setState({
    openTabs: [
      {
        id: "tab1",
        scope: "repo",
        pipeline: makePipeline(loops),
        prompts: {},
        diagnostics: [],
        dirty: false,
        externalDirty: false,
      },
    ],
    activeTabId: "tab1",
    selection: { kind: "region", id: null, regionId },
  });
}

const reviewLoop: LoopRegion = {
  id: "review_loop",
  kind: "bounded",
  members: ["impl", "rev"],
  max_iter: 3,
};

describe("RegionInspector (#150)", () => {
  beforeEach(() => {
    useEditStore.setState({
      openTabs: [],
      activeTabId: null,
      selection: { kind: "none", id: null },
    });
  });

  it("renders nothing when the selection is not a region", () => {
    useEditStore.setState({
      openTabs: [
        {
          id: "tab1",
          scope: "repo",
          pipeline: makePipeline([reviewLoop]),
          prompts: {},
          diagnostics: [],
          dirty: false,
          externalDirty: false,
        },
      ],
      activeTabId: "tab1",
      selection: { kind: "none", id: null },
    });
    const { container } = render(<RegionInspector />);
    expect(container.firstChild).toBeNull();
  });

  it("shows the selected region's id and members", () => {
    selectRegion([reviewLoop], "review_loop");
    render(<RegionInspector />);
    expect(screen.getByText("review_loop")).toBeInTheDocument();
    // Member node names are listed.
    expect(screen.getByText(/implementer/)).toBeInTheDocument();
    expect(screen.getByText(/reviewer/)).toBeInTheDocument();
  });

  it("exposes max_iter in an editable field seeded with the current bound", () => {
    selectRegion([reviewLoop], "review_loop");
    render(<RegionInspector />);
    const input = screen.getByTestId("region-max-iter");
    expect(input).toHaveValue(3);
  });

  it("commits a new max_iter to the region via updateRegion", () => {
    selectRegion([reviewLoop], "review_loop");
    render(<RegionInspector />);
    const input = screen.getByTestId("region-max-iter");
    fireEvent.change(input, { target: { value: "8" } });
    const tab = useEditStore.getState().openTabs[0];
    expect(tab.pipeline.loops!.find((r) => r.id === "review_loop")!.max_iter).toBe(8);
  });
});
