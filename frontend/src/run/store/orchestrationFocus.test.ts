import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useOrchestrationFocus, useOrchestrationFocusTab } from "./orchestrationFocus";

beforeEach(() => {
  useOrchestrationFocus.setState({ request: null, applied: 0 });
});

describe("opening a node's Orchestration tab from its card", () => {
  it("switches the node's panel once, whether it was mounted before or after", () => {
    const mounted = vi.fn();
    const other = vi.fn();
    renderHook(() => useOrchestrationFocusTab("r1", "dispatch", mounted));
    renderHook(() => useOrchestrationFocusTab("r1", "planner", other));
    act(() => useOrchestrationFocus.getState().open("r1", "dispatch"));
    expect(mounted).toHaveBeenCalledOnce();
    expect(other).not.toHaveBeenCalled();

    // Applied: a panel of the same node mounted later stays where it is.
    const later = vi.fn();
    renderHook(() => useOrchestrationFocusTab("r1", "dispatch", later));
    expect(later).not.toHaveBeenCalled();
  });

  it("reaches a panel that mounts on the selection the click made", () => {
    act(() => useOrchestrationFocus.getState().open("r1", "dispatch"));
    const apply = vi.fn();
    renderHook(() => useOrchestrationFocusTab("r1", "dispatch", apply));
    expect(apply).toHaveBeenCalledOnce();
  });

  it("names the Run as well as the node", () => {
    act(() => useOrchestrationFocus.getState().open("r2", "dispatch"));
    const apply = vi.fn();
    renderHook(() => useOrchestrationFocusTab("r1", "dispatch", apply));
    expect(apply).not.toHaveBeenCalled();
  });
});
