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
    exhausted: false,
    runId: null,
    width: 400,
    height: 200,
    // Every test below drives the header or the exhausted badge, which live on
    // the chrome layer (#455). The box layer is covered by its own describe.
    layer: "chrome",
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

describe("LoopRegionNode header (slim card #149 / #150)", () => {
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

  it("renders the bound read-only — no inline max_iter editor on the canvas", () => {
    // The bound is editable only in the RegionInspector: the canvas header must
    // carry no <input>, honouring the slim-card rule (#149). The earlier inline
    // header editor (#150) was removed.
    seedStore([reviewLoop]);
    render(
      <Wrapper>
        <LoopRegionNode {...props({ counterText: "max 3" })} />
      </Wrapper>,
    );
    const header = screen.getByTestId("loop-region-header");
    expect(screen.queryByTestId("region-header-max-iter")).toBeNull();
    expect(screen.queryByTestId("region-iter-prefix")).toBeNull();
    expect(header.querySelector("input")).toBeNull();
    expect(header).toHaveTextContent("max 3");
  });

  it("keeps the live ↻ i/N counter read-only during a run", () => {
    // On a live run the header reads `↻ 2/3` as plain text — no editable bound,
    // so the live-counter display the loop-region scenario asserts is preserved.
    seedStore([reviewLoop]);
    render(
      <Wrapper>
        <LoopRegionNode {...props({ counterText: "2/3" })} />
      </Wrapper>,
    );
    const header = screen.getByTestId("loop-region-header");
    expect(header).toHaveTextContent("2/3");
    expect(screen.queryByTestId("region-header-max-iter")).toBeNull();
  });

  it("does not show the region id on the canvas header", () => {
    // The id lives in the RegionInspector only; the slim card shows it nowhere
    // on the canvas (the `loop-region-name` span was removed).
    seedStore([reviewLoop]);
    render(
      <Wrapper>
        <LoopRegionNode
          {...props({ regionId: "loop-473d6a4234e27f9a", counterText: "max 3" })}
        />
      </Wrapper>,
    );
    const header = screen.getByTestId("loop-region-header");
    expect(header.querySelector(".loop-region-name")).toBeNull();
    expect(header).not.toHaveTextContent("loop-473d6a4234e27f9a");
  });

  it("renders no inline editor for a collection region either", () => {
    seedStore([
      { id: "per-issue", kind: "collection", members: ["impl"], over: "issues" },
    ]);
    render(
      <Wrapper>
        <LoopRegionNode
          {...props({ regionId: "per-issue", kind: "collection", counterText: "over issues" })}
        />
      </Wrapper>,
    );
    expect(screen.queryByTestId("region-header-max-iter")).toBeNull();
    expect(screen.getByTestId("loop-region-header")).toHaveTextContent("over issues");
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

/**
 * #455: the two layers are exclusive. The box draws the grouping rectangle and
 * nothing interactive; the chrome draws the chips and no rectangle. If either
 * leaked into the other, the fix would be undone — chrome under a card again, or
 * a filled rectangle painted over the cards it is supposed to sit behind.
 */
describe("LoopRegionNode — box / chrome layer split (#455)", () => {
  beforeEach(() => {
    useEditStore.setState({
      openTabs: [],
      activeTabId: null,
      selection: { kind: "none", id: null },
    });
  });

  it("the box layer draws the rectangle and NO interactive chrome", () => {
    seedStore([reviewLoop]);
    render(
      <Wrapper>
        <LoopRegionNode {...props({ layer: "box", exhausted: true, runId: "run-1" })} />
      </Wrapper>,
    );
    const box = screen.getByTestId("loop-region");
    expect(box.style.border).toContain("dashed");
    expect(box.style.background).not.toBe("");
    expect(screen.queryByTestId("loop-region-header")).toBeNull();
    expect(screen.queryByTestId("loop-region-block")).toBeNull();
    expect(screen.queryByTestId("loop-region-route-from-manager")).toBeNull();
  });

  it("the chrome layer draws the chips and NO rectangle over the cards", () => {
    seedStore([reviewLoop]);
    render(
      <Wrapper>
        <LoopRegionNode {...props({ layer: "chrome", exhausted: true, runId: "run-1" })} />
      </Wrapper>,
    );
    expect(screen.getByTestId("loop-region-header")).toBeTruthy();
    expect(screen.getByTestId("loop-region-block")).toBeTruthy();
    // No `loop-region` box: the chrome covers the member cards, so a border or a
    // fill here would hide them.
    expect(screen.queryByTestId("loop-region")).toBeNull();
    const chrome = screen.getByTestId("loop-region-chrome");
    expect(chrome.style.border).toBe("");
    expect(chrome.style.background).toBe("");
  });

  it("keeps the chrome container itself pass-through, only the chips clickable", () => {
    seedStore([reviewLoop]);
    render(
      <Wrapper>
        <LoopRegionNode {...props({ layer: "chrome" })} />
      </Wrapper>,
    );
    // The container spans the whole box and now sits ABOVE the member cards, so
    // it must not swallow their clicks (#167, the other way round).
    expect(screen.getByTestId("loop-region-chrome").className).toContain("pointer-events-none");
    expect(screen.getByTestId("loop-region-header").className).toContain("pointer-events-auto");
  });
});
