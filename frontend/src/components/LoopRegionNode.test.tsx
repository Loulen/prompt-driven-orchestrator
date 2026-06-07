import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ReactFlowProvider } from "@xyflow/react";
import type { Node, NodeProps } from "@xyflow/react";
import { LoopRegionNode, type LoopRegionNodeData } from "./LoopRegionNode";
import { useEditStore } from "../stores/editStore";
import { endRegion, bumpRegion } from "../api";
import type { PipelineDef, LoopRegion, NodeDef } from "../types";

vi.mock("../api", () => ({
  endRegion: vi.fn().mockResolvedValue(undefined),
  bumpRegion: vi.fn().mockResolvedValue(undefined),
}));

const mockEndRegion = vi.mocked(endRegion);
const mockBumpRegion = vi.mocked(bumpRegion);

function Wrapper({ children }: { children: React.ReactNode }) {
  return <ReactFlowProvider>{children}</ReactFlowProvider>;
}

function makeNode(id: string): NodeDef {
  return {
    id,
    name: id,
    type: "code-mutating",
    inputs: [],
    outputs: [{ name: "out", repeated: false, side: "right" }],
    interactive: false,
  };
}

function seedStore(loops: LoopRegion[]) {
  const pipeline: PipelineDef = {
    name: "p",
    variables: {},
    nodes: [makeNode("impl"), makeNode("rev")],
    edges: [],
    loops,
  };
  useEditStore.setState({
    openTabs: [
      {
        id: "tab1",
        scope: "repo",
        pipeline,
        prompts: {},
        diagnostics: [],
        dirty: false,
        externalDirty: false,
      },
    ],
    activeTabId: "tab1",
    selection: { kind: "none", id: null },
  });
}

function props(data: Partial<LoopRegionNodeData>): NodeProps<Node<LoopRegionNodeData>> {
  const full: LoopRegionNodeData = {
    regionId: "review_loop",
    kind: "bounded",
    counterText: "max 3",
    iterPrefix: "max ",
    maxIter: 3,
    exhausted: false,
    runId: null,
    width: 400,
    height: 200,
  };
  return {
    id: "region-review_loop",
    data: { ...full, ...data },
    selected: false,
    type: "loopRegion",
    dragging: false,
    zIndex: 0,
    isConnectable: false,
    positionAbsoluteX: 0,
    positionAbsoluteY: 0,
  } as unknown as NodeProps<Node<LoopRegionNodeData>>;
}

const reviewLoop: LoopRegion = {
  id: "review_loop",
  kind: "bounded",
  members: ["impl", "rev"],
  max_iter: 3,
};

describe("LoopRegionNode header (#150)", () => {
  beforeEach(() => {
    useEditStore.setState({
      openTabs: [],
      activeTabId: null,
      selection: { kind: "none", id: null },
    });
  });

  it("selects the region when its header is clicked", () => {
    seedStore([reviewLoop]);
    render(
      <Wrapper>
        <LoopRegionNode {...props({})} />
      </Wrapper>,
    );
    fireEvent.click(screen.getByTestId("loop-region-header"));
    const sel = useEditStore.getState().selection;
    expect(sel.kind).toBe("region");
    expect(sel.regionId).toBe("review_loop");
  });

  it("edits max_iter inline from the header for a bounded region", () => {
    seedStore([reviewLoop]);
    render(
      <Wrapper>
        <LoopRegionNode {...props({})} />
      </Wrapper>,
    );
    const input = screen.getByTestId("region-header-max-iter");
    fireEvent.change(input, { target: { value: "6" } });
    const tab = useEditStore.getState().openTabs[0];
    expect(tab.pipeline.loops!.find((r) => r.id === "review_loop")!.max_iter).toBe(6);
  });

  it("shows the live lap prefix before the editable max during a run (preserves the ↻ i/N counter)", () => {
    // On a live run the header reads `↻ 2/3`: the `2/` is the live lap (read-
    // only progress) and the `3` is the editable bound. Editing the bound must
    // not erase the live-counter display the loop-region scenario asserts.
    seedStore([reviewLoop]);
    render(
      <Wrapper>
        <LoopRegionNode {...props({ iterPrefix: "2/", counterText: "2/3" })} />
      </Wrapper>,
    );
    expect(screen.getByTestId("region-iter-prefix")).toHaveTextContent("2/");
    expect(screen.getByTestId("region-header-max-iter")).toHaveValue(3);
  });

  it("shows no inline max_iter editor for a collection region", () => {
    seedStore([
      { id: "per-issue", kind: "collection", members: ["impl"], over: "issues" },
    ]);
    render(
      <Wrapper>
        <LoopRegionNode
          {...props({ regionId: "per-issue", kind: "collection", counterText: "over issues", iterPrefix: "over issues", maxIter: null })}
        />
      </Wrapper>,
    );
    expect(screen.queryByTestId("region-header-max-iter")).toBeNull();
  });
});

describe("LoopRegionNode — route from manager (#152)", () => {
  beforeEach(() => {
    mockEndRegion.mockClear();
    mockBumpRegion.mockClear();
  });

  it("offers no manager affordance while the region is not exhausted", () => {
    render(
      <Wrapper>
        <LoopRegionNode {...props({ exhausted: false, runId: "run-1" })} />
      </Wrapper>,
    );
    expect(
      screen.queryByTestId("loop-region-route-from-manager"),
    ).toBeNull();
  });

  it("offers 'route from manager' on an exhausted-unrouted region of a live run", () => {
    render(
      <Wrapper>
        <LoopRegionNode {...props({ exhausted: true, runId: "run-1" })} />
      </Wrapper>,
    );
    const btn = screen.getByTestId("loop-region-route-from-manager");
    expect(btn).toBeTruthy();
    expect(btn.textContent?.toLowerCase()).toContain("route from manager");
  });

  it("ends the region by id when the affordance is triggered", () => {
    render(
      <Wrapper>
        <LoopRegionNode
          {...props({ exhausted: true, runId: "run-1", regionId: "review_loop" })}
        />
      </Wrapper>,
    );
    fireEvent.click(screen.getByTestId("loop-region-route-from-manager"));
    expect(mockEndRegion).toHaveBeenCalledWith("run-1", "review_loop");
  });

  it("shows no affordance with no live run id (template view)", () => {
    render(
      <Wrapper>
        <LoopRegionNode {...props({ exhausted: true, runId: null })} />
      </Wrapper>,
    );
    expect(
      screen.queryByTestId("loop-region-route-from-manager"),
    ).toBeNull();
  });
});
