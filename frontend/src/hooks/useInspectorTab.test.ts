import { renderHook, act } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { useInspectorTab } from "./useInspectorTab";

describe("useInspectorTab", () => {
  // After #271 the default depends only on whether the tab is a run tab; run
  // status no longer gates it. The status-named cases below intentionally
  // exercise the same code path — they document that ALL run statuses (live
  // and terminal) default to the Run tab, and guard against a status gate
  // being re-introduced.
  it("defaults to 'run' on a live run", () => {
    const { result } = renderHook(() => useInspectorTab("tab-1", true));
    expect(result.current.activeTab).toBe("run");
  });

  it("defaults to 'run' on a completed run (#271)", () => {
    const { result } = renderHook(() => useInspectorTab("tab-1", true));
    expect(result.current.activeTab).toBe("run");
  });

  it("defaults to 'run' on a failed/terminal run (#271)", () => {
    const { result } = renderHook(() => useInspectorTab("tab-1", true));
    expect(result.current.activeTab).toBe("run");
  });

  it("defaults to 'run' on an archived run (#271)", () => {
    const { result } = renderHook(() => useInspectorTab("tab-1", true));
    expect(result.current.activeTab).toBe("run");
  });

  it("defaults to 'edit' when not editing a run (library draft)", () => {
    const { result } = renderHook(() => useInspectorTab("tab-1", false));
    expect(result.current.activeTab).toBe("edit");
  });

  it("user override is sticky across re-renders", () => {
    const { result, rerender } = renderHook(() =>
      useInspectorTab("tab-1", true),
    );
    expect(result.current.activeTab).toBe("run");

    act(() => result.current.setActiveTab("edit"));
    expect(result.current.activeTab).toBe("edit");

    rerender();
    expect(result.current.activeTab).toBe("edit");
  });

  it("resets override when pipelineKey changes", () => {
    let key = "tab-1";
    const { result, rerender } = renderHook(() => useInspectorTab(key, true));

    act(() => result.current.setActiveTab("edit"));
    expect(result.current.activeTab).toBe("edit");

    key = "tab-2";
    rerender();
    expect(result.current.activeTab).toBe("run");
  });
});
