import { renderHook, act } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { useInspectorTab } from "./useInspectorTab";

describe("useInspectorTab", () => {
  it("defaults to 'run' when editing an active run", () => {
    const { result } = renderHook(() =>
      useInspectorTab("tab-1", "running", true),
    );
    expect(result.current.activeTab).toBe("run");
  });

  it("defaults to 'run' for awaiting_user status", () => {
    const { result } = renderHook(() =>
      useInspectorTab("tab-1", "awaiting_user", true),
    );
    expect(result.current.activeTab).toBe("run");
  });

  it("defaults to 'run' for halted status", () => {
    const { result } = renderHook(() =>
      useInspectorTab("tab-1", "halted", true),
    );
    expect(result.current.activeTab).toBe("run");
  });

  it("defaults to 'edit' when not editing a run", () => {
    const { result } = renderHook(() =>
      useInspectorTab("tab-1", null, false),
    );
    expect(result.current.activeTab).toBe("edit");
  });

  it("defaults to 'edit' for completed run", () => {
    const { result } = renderHook(() =>
      useInspectorTab("tab-1", "completed", true),
    );
    expect(result.current.activeTab).toBe("edit");
  });

  it("defaults to 'edit' for archived run", () => {
    const { result } = renderHook(() =>
      useInspectorTab("tab-1", "archived", true),
    );
    expect(result.current.activeTab).toBe("edit");
  });

  it("user override is sticky across re-renders", () => {
    const { result, rerender } = renderHook(() =>
      useInspectorTab("tab-1", "running", true),
    );
    expect(result.current.activeTab).toBe("run");

    act(() => result.current.setActiveTab("edit"));
    expect(result.current.activeTab).toBe("edit");

    rerender();
    expect(result.current.activeTab).toBe("edit");
  });

  it("resets override when pipelineKey changes", () => {
    let key = "tab-1";
    const { result, rerender } = renderHook(() =>
      useInspectorTab(key, "running", true),
    );

    act(() => result.current.setActiveTab("edit"));
    expect(result.current.activeTab).toBe("edit");

    key = "tab-2";
    rerender();
    expect(result.current.activeTab).toBe("run");
  });

  it("defaults to 'edit' when isEditingRun is false even with active status", () => {
    const { result } = renderHook(() =>
      useInspectorTab("tab-1", "running", false),
    );
    expect(result.current.activeTab).toBe("edit");
  });
});
