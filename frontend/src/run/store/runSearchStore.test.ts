import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RunTranscriptHit } from "../../shared/generated/RunTranscriptHit";
import { useFocusTab, useOnHitChosen, useRunSearchStore } from "./runSearchStore";

const HIT: RunTranscriptHit = {
  node_id: "builder",
  iter: 2,
  hit: { kind: "message", index: 4, speaker: "agent", snippet: { before: "", matched: "x", after: "" } },
};

beforeEach(() => {
  useRunSearchStore.setState({ runId: null, query: "", result: null, focus: null, tabApplied: 0 });
});

describe("the hit chosen in the Run's search", () => {
  it("switches the node's panel once, at the hit's iteration", () => {
    const apply = vi.fn();
    const other = vi.fn();
    renderHook(() => useFocusTab("r1", "builder", apply));
    renderHook(() => useFocusTab("r1", "reviewer", other));
    act(() => useRunSearchStore.getState().choose("r1", HIT));
    expect(apply).toHaveBeenCalledExactlyOnceWith(2);
    expect(other).not.toHaveBeenCalled();

    // A panel of the same node mounted later does not switch again.
    const later = vi.fn();
    renderHook(() => useFocusTab("r1", "builder", later));
    expect(later).not.toHaveBeenCalled();

    // Choosing it again does.
    act(() => useRunSearchStore.getState().choose("r1", HIT));
    expect(apply).toHaveBeenCalledTimes(2);
  });

  it("is announced to who listens, not replayed on mount", () => {
    act(() => useRunSearchStore.getState().choose("r1", HIT));
    const onChosen = vi.fn();
    renderHook(() => useOnHitChosen(onChosen));
    expect(onChosen).not.toHaveBeenCalled();
    act(() => useRunSearchStore.getState().choose("r1", HIT));
    expect(onChosen).toHaveBeenCalledOnce();
  });
});

describe("closing the search", () => {
  it("lets go of the chosen hit", () => {
    useRunSearchStore.getState().open("r1");
    useRunSearchStore.getState().choose("r1", HIT);
    expect(useRunSearchStore.getState().focus).not.toBeNull();
    useRunSearchStore.getState().close();
    expect(useRunSearchStore.getState().focus).toBeNull();
  });
});
