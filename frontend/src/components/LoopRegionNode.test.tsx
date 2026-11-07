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
    type: "agent",
    inputs: [],
    outputs: [{ name: "out" }],
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

describe("LoopRegionNode header — Coût de boucle (#1015)", () => {
  beforeEach(() => seedStore([reviewLoop]));

  it.each([
    ["~$1.20", "estimate"],
    ["~$1.20†", "partial (lower bound)"],
    ["—", "unknown"],
    ["$1.20", "exact reported dollars"],
  ])("shows `%s` (%s) after the counter", (costText) => {
    render(
      <Wrapper>
        <LoopRegionNode {...props({ counterText: "3/5", costText, costTitle: "Loop cost, laps 1–3: …" })} />
      </Wrapper>,
    );
    const header = screen.getByTestId("loop-region-header");
    expect(header).toHaveTextContent(`3/5·${costText}`);
    expect(screen.getByTestId("loop-region-cost")).toHaveTextContent(costText);
    expect(header).toHaveAttribute("title", "Loop cost, laps 1–3: …");
  });

  it("shows no cost in a template view (no run)", () => {
    render(
      <Wrapper>
        <LoopRegionNode {...props({ counterText: "max 5" })} />
      </Wrapper>,
    );
    expect(screen.queryByTestId("loop-region-cost")).toBeNull();
    expect(screen.getByTestId("loop-region-header")).not.toHaveAttribute("title");
  });

  it("shows the cost on a collection region too", () => {
    render(
      <Wrapper>
        <LoopRegionNode
          {...props({ kind: "collection", counterText: "2/4 items", costText: "~$0.8400" })}
        />
      </Wrapper>,
    );
    expect(screen.getByTestId("loop-region-header")).toHaveTextContent("⇉2/4 items·~$0.8400");
  });
});

describe("LoopRegionNode — Plafond de coût (#1016)", () => {
  beforeEach(() => {
    mockEndRegion.mockClear();
    seedStore([{ ...reviewLoop, max_cost_usd: 5 }]);
  });

  it("shows `/ $cap` after the loop cost", () => {
    render(
      <Wrapper>
        <LoopRegionNode {...props({ counterText: "3/5", costText: "~$1.20 / $5" })} />
      </Wrapper>,
    );
    expect(screen.getByTestId("loop-region-header")).toHaveTextContent("3/5·~$1.20 / $5");
    expect(screen.getByTestId("loop-region-cost")).not.toHaveAttribute("data-warn");
  });

  it("turns only the cost text amber at 80 % of the cap", () => {
    render(
      <Wrapper>
        <LoopRegionNode {...props({ counterText: "4/5", costText: "~$4.31 / $5", costWarn: true })} />
      </Wrapper>,
    );
    const cost = screen.getByTestId("loop-region-cost");
    expect(cost).toHaveAttribute("data-warn", "true");
    expect(cost.style.color).toBe("var(--color-st-await)");
    expect(screen.getByTestId("loop-region-header").style.color).toBe("var(--color-acc)");
    expect(screen.queryByTestId("loop-region-budget")).toBeNull();
  });

  it("renders « budget reached » in amber, distinct from exhausted", () => {
    render(
      <Wrapper>
        <LoopRegionNode
          {...props({ counterText: "4/5", costText: "~$5.12 / $5", budgetReached: true, runId: "run-1" })}
        />
      </Wrapper>,
    );
    const chip = screen.getByTestId("loop-region-budget");
    expect(chip).toHaveTextContent("budget reached");
    expect(chip.style.color).toBe("var(--color-st-await)");
    expect(screen.getByTestId("loop-region-header").style.color).toBe("var(--color-st-await)");
    expect(screen.getByTestId("loop-region-budget-glyph")).toBeTruthy();
    // Never the exhausted chip, never red/orange.
    expect(screen.queryByTestId("loop-region-block")).toBeNull();
    expect(chip.style.color).not.toContain("st-blocked");
    expect(chip.style.color).not.toContain("st-failed");
  });

  it("tints the box amber when the budget is reached", () => {
    render(
      <Wrapper>
        <LoopRegionNode {...props({ layer: "box", budgetReached: true, runId: "run-1" })} />
      </Wrapper>,
    );
    const box = screen.getByTestId("loop-region");
    expect(box.style.background).toBe("var(--color-st-await-bg)");
    expect(box.style.border).toContain("var(--color-st-await)");
  });

  it("`raise cap` opens the inspector on max_cost_usd", () => {
    render(
      <Wrapper>
        <LoopRegionNode {...props({ budgetReached: true, runId: "run-1" })} />
      </Wrapper>,
    );
    fireEvent.click(screen.getByTestId("loop-region-raise-cap"));
    expect(useEditStore.getState().selection).toEqual({
      kind: "region",
      id: null,
      regionId: "review_loop",
      focusField: "max_cost_usd",
    });
  });

  it("`raise cap` unlocks « Edit for this run » on a followed run tab (#1017)", () => {
    // A run tab in « pilotage »: locked until the user edits for this run.
    useEditStore.setState((st) => ({
      openTabs: st.openTabs.map((t) => ({ ...t, scope: "run", runId: "run-1", runEditing: false })),
    }));
    render(
      <Wrapper>
        <LoopRegionNode {...props({ budgetReached: true, runId: "run-1" })} />
      </Wrapper>,
    );
    fireEvent.click(screen.getByTestId("loop-region-raise-cap"));
    expect(useEditStore.getState().openTabs[0].runEditing).toBe(true);
    expect(useEditStore.getState().selection.focusField).toBe("max_cost_usd");
  });

  it("`end region` ends the region by id", () => {
    render(
      <Wrapper>
        <LoopRegionNode {...props({ budgetReached: true, runId: "run-1" })} />
      </Wrapper>,
    );
    fireEvent.click(screen.getByTestId("loop-region-end-region"));
    expect(mockEndRegion).toHaveBeenCalledWith("run-1", "review_loop");
  });
});
