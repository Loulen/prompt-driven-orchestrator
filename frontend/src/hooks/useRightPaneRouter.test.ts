import { renderHook } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { useRightPaneRouter } from "./useRightPaneRouter";
import type { RightPaneRouterArgs } from "./useRightPaneRouter";
import type { Selection } from "../stores/editStore";
import type { Trigger, PipelineListEntry, RunState } from "../types";

const NO_SEL: Selection = { kind: "none", id: null };
const nodeSel = (id: string): Selection => ({ kind: "node", id });

function trigger(id: string, pipeline_id: string): Trigger {
  return { id, pipeline_id } as unknown as Trigger;
}

function pipeline(
  id: string,
  prompt_required?: boolean,
): PipelineListEntry {
  return { id, prompt_required } as unknown as PipelineListEntry;
}

function makeRun(over: Partial<RunState>): RunState {
  return {
    run_id: "r1",
    status: "running",
    nodes: {},
    node_defs: [],
    ...over,
  } as unknown as RunState;
}

function baseArgs(over: Partial<RightPaneRouterArgs> = {}): RightPaneRouterArgs {
  return {
    selection: NO_SEL,
    editActiveTabId: null,
    hasEditTab: false,
    selectedTriggerId: null,
    setSelectedTriggerId: vi.fn(),
    triggerOpenedTabId: null,
    infoPanelOpen: false,
    setInfoPanelOpen: vi.fn(),
    triggers: [],
    pipelines: [],
    selectedRun: null,
    ...over,
  };
}

describe("useRightPaneRouter — pane-owner precedence (#247)", () => {
  it("info wins over everything", () => {
    const { result } = renderHook(() =>
      useRightPaneRouter(
        baseArgs({
          infoPanelOpen: true,
          selectedTriggerId: "t1",
          triggers: [trigger("t1", "p1")],
          hasEditTab: true,
        }),
      ),
    );
    expect(result.current.paneOwner).toBe("info");
  });

  it("a selected Trigger wins over a persistent edit tab (#247)", () => {
    const { result } = renderHook(() =>
      useRightPaneRouter(
        baseArgs({
          selectedTriggerId: "t1",
          triggers: [trigger("t1", "p1")],
          hasEditTab: true,
        }),
      ),
    );
    expect(result.current.paneOwner).toBe("trigger");
    expect(result.current.selectedTrigger?.id).toBe("t1");
  });

  it("editTab when an edit tab is open and no trigger/info", () => {
    const { result } = renderHook(() =>
      useRightPaneRouter(baseArgs({ hasEditTab: true })),
    );
    expect(result.current.paneOwner).toBe("editTab");
  });

  it("selectedNode is the legacy fallback", () => {
    const { result } = renderHook(() => useRightPaneRouter(baseArgs()));
    expect(result.current.paneOwner).toBe("selectedNode");
  });

  it("a deleted Trigger resolves to null and yields no phantom trigger pane", () => {
    const { result } = renderHook(() =>
      useRightPaneRouter(
        baseArgs({ selectedTriggerId: "gone", triggers: [], hasEditTab: true }),
      ),
    );
    expect(result.current.selectedTrigger).toBeNull();
    expect(result.current.paneOwner).toBe("editTab");
  });
});

describe("useRightPaneRouter — #320 canvas-reclaim reconciliation", () => {
  it("a genuine canvas selection clears the selected Trigger", () => {
    const setSelectedTriggerId = vi.fn();
    const { rerender } = renderHook(
      (args: RightPaneRouterArgs) => useRightPaneRouter(args),
      {
        initialProps: baseArgs({
          selectedTriggerId: "t1",
          triggers: [trigger("t1", "p1")],
          selection: NO_SEL,
          editActiveTabId: "p1",
          triggerOpenedTabId: "p1",
          setSelectedTriggerId,
        }),
      },
    );
    // Focus a node on the trigger's own tab: selectionKind !== "none" ⇒ reclaim.
    rerender(
      baseArgs({
        selectedTriggerId: "t1",
        triggers: [trigger("t1", "p1")],
        selection: nodeSel("n1"),
        editActiveTabId: "p1",
        triggerOpenedTabId: "p1",
        setSelectedTriggerId,
      }),
    );
    expect(setSelectedTriggerId).toHaveBeenCalledTimes(1);
    expect(setSelectedTriggerId).toHaveBeenCalledWith(null);
  });

  it("the Trigger's OWN openPipeline landing does NOT clear it (#320)", () => {
    const setSelectedTriggerId = vi.fn();
    const { rerender } = renderHook(
      (args: RightPaneRouterArgs) => useRightPaneRouter(args),
      {
        initialProps: baseArgs({
          selectedTriggerId: "t1",
          triggers: [trigger("t1", "p1")],
          selection: NO_SEL,
          editActiveTabId: null,
          triggerOpenedTabId: null,
          setSelectedTriggerId,
        }),
      },
    );
    // The trigger opens its pipeline: active tab becomes p1, nothing selected.
    rerender(
      baseArgs({
        selectedTriggerId: "t1",
        triggers: [trigger("t1", "p1")],
        selection: NO_SEL,
        editActiveTabId: "p1",
        triggerOpenedTabId: "p1",
        setSelectedTriggerId,
      }),
    );
    expect(setSelectedTriggerId).not.toHaveBeenCalled();
  });
});

describe("useRightPaneRouter — #385 info auto-close reconciliation", () => {
  it("closes the info overlay when the active tab changes", () => {
    const setInfoPanelOpen = vi.fn();
    const { rerender } = renderHook(
      (args: RightPaneRouterArgs) => useRightPaneRouter(args),
      {
        initialProps: baseArgs({
          infoPanelOpen: true,
          editActiveTabId: "p1",
          setInfoPanelOpen,
        }),
      },
    );
    rerender(
      baseArgs({
        infoPanelOpen: true,
        editActiveTabId: "p2",
        setInfoPanelOpen,
      }),
    );
    expect(setInfoPanelOpen).toHaveBeenCalledTimes(1);
    expect(setInfoPanelOpen).toHaveBeenCalledWith(false);
  });

  it("keeps the info overlay open while the active tab is unchanged", () => {
    const setInfoPanelOpen = vi.fn();
    const { rerender } = renderHook(
      (args: RightPaneRouterArgs) => useRightPaneRouter(args),
      {
        initialProps: baseArgs({
          infoPanelOpen: true,
          editActiveTabId: "p1",
          setInfoPanelOpen,
        }),
      },
    );
    rerender(
      baseArgs({
        infoPanelOpen: true,
        editActiveTabId: "p1",
        setInfoPanelOpen,
      }),
    );
    expect(setInfoPanelOpen).not.toHaveBeenCalled();
  });
});

describe("useRightPaneRouter — triggerPromptRequired (#351)", () => {
  it("defaults to true when the flag is absent", () => {
    const { result } = renderHook(() =>
      useRightPaneRouter(
        baseArgs({
          selectedTriggerId: "t1",
          triggers: [trigger("t1", "p1")],
          pipelines: [pipeline("p1")],
        }),
      ),
    );
    expect(result.current.triggerPromptRequired).toBe(true);
  });

  it("is false when the pipeline sets prompt_required = false", () => {
    const { result } = renderHook(() =>
      useRightPaneRouter(
        baseArgs({
          selectedTriggerId: "t1",
          triggers: [trigger("t1", "p1")],
          pipelines: [pipeline("p1", false)],
        }),
      ),
    );
    expect(result.current.triggerPromptRequired).toBe(false);
  });

  it("is false when the pipeline can't be found (dangling reference)", () => {
    const { result } = renderHook(() =>
      useRightPaneRouter(
        baseArgs({
          selectedTriggerId: "t1",
          triggers: [trigger("t1", "p1")],
          pipelines: [],
        }),
      ),
    );
    expect(result.current.triggerPromptRequired).toBe(false);
  });
});

describe("useRightPaneRouter — runNode synthesis (#204)", () => {
  it("returns the existing NodeState when the run already tracks the node", () => {
    const existing = { node_id: "n1", status: "running" } as never;
    const { result } = renderHook(() =>
      useRightPaneRouter(
        baseArgs({
          selection: nodeSel("n1"),
          selectedRun: makeRun({ nodes: { n1: existing } }),
        }),
      ),
    );
    expect(result.current.runNode).toBe(existing);
  });

  it("synthesizes a pending node on a live run for an unscheduled node", () => {
    const { result } = renderHook(() =>
      useRightPaneRouter(
        baseArgs({
          selection: nodeSel("n2"),
          selectedRun: makeRun({
            status: "running",
            nodes: {},
            node_defs: [{ id: "n2", node_type: "code-mutating" }] as never,
          }),
        }),
      ),
    );
    expect(result.current.runNode).toMatchObject({
      node_id: "n2",
      status: "pending",
    });
  });

  it("returns null on a terminal run", () => {
    const { result } = renderHook(() =>
      useRightPaneRouter(
        baseArgs({
          selection: nodeSel("n2"),
          selectedRun: makeRun({
            status: "completed",
            nodes: {},
            node_defs: [{ id: "n2", node_type: "code-mutating" }] as never,
          }),
        }),
      ),
    );
    expect(result.current.runNode).toBeNull();
  });

  it("returns null for start/end pseudo-nodes", () => {
    const { result } = renderHook(() =>
      useRightPaneRouter(
        baseArgs({
          selection: nodeSel("start"),
          selectedRun: makeRun({
            status: "running",
            nodes: {},
            node_defs: [{ id: "start", node_type: "start" }] as never,
          }),
        }),
      ),
    );
    expect(result.current.runNode).toBeNull();
  });
});
